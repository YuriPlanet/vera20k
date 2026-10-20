#!/usr/bin/env python3
"""Stage a native Rust iOS executable and local retail assets as an app and IPA.

This does not compile, register devices, request profiles, install, or upload an app.
The default executable target is aarch64-apple-ios. Use --simulator explicitly for
an aarch64-apple-ios-sim executable and a separate simulator output directory.
"""

import argparse
import datetime
import fnmatch
import hashlib
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile
import zipfile


OWNER = "vera20k.tools.ios.package.v1"
MARKER = "package-manifest.json"
DISPLAY_NAME = "王二火大"
APP_NAME = DISPLAY_NAME + ".app"
EXECUTABLE_NAME = "vera20k"
APP_ICON = Path(__file__).resolve().parents[2] / "art" / "branding" / "wannafire-app-icon.png"


class PackageError(Exception):
    pass


def run(argv):
    result = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        detail = result.stderr.decode(errors="replace").strip()
        raise PackageError("{} failed: {}".format(argv[0], detail))
    return result.stdout


def absolute_path(value):
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise argparse.ArgumentTypeError("use an absolute path")
    return path


def version(value):
    if not re.fullmatch(r"\d+\.\d+(?:\.\d+)?", value):
        raise argparse.ArgumentTypeError("expected a version such as 16.0")
    return value


def version_tuple(value):
    parts = [int(part) for part in value.split(".")]
    return tuple(parts + [0] * (3 - len(parts)))


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def contains(parent, child):
    return parent == child or parent in child.parents


def check_output(output, replace, expected_platform):
    if output.is_symlink():
        raise PackageError("output must not be a symlink")
    if not output.exists():
        return
    if not output.is_dir():
        raise PackageError("output already exists and is not a directory")
    if not any(output.iterdir()):
        return
    try:
        manifest = json.loads((output / MARKER).read_text())
    except (OSError, ValueError):
        raise PackageError("refusing a nonempty output without this tool's manifest")
    if manifest.get("owner") != OWNER:
        raise PackageError("output belongs to another tool")
    if not replace:
        raise PackageError("output already contains a package; use --replace to rebuild it")
    if manifest.get("executable", {}).get("platform") != expected_platform:
        raise PackageError("refusing to replace a package for a different platform; use a separate output")


def inspect_binary(executable, minimum_ios, simulator=False):
    description = run(["/usr/bin/file", "-b", str(executable)]).decode().strip()
    if "Mach-O 64-bit executable arm64" not in description:
        raise PackageError("executable must be a thin arm64 Mach-O executable: " + description)
    build = run(["xcrun", "vtool", "-show-build", str(executable)]).decode()
    platform = "IOSSIMULATOR" if simulator else "IOS"
    if not re.search(r"\bplatform\s+" + platform + r"\b", build):
        raise PackageError("executable has the wrong platform ({} required)".format(platform))
    match = re.search(r"\bminos\s+([\d.]+)", build)
    if not match:
        raise PackageError("could not read the executable's minimum iOS version")
    binary_minimum = match.group(1)
    if version_tuple(binary_minimum) > version_tuple(minimum_ios):
        raise PackageError("binary requires iOS {}; raise --minimum-ios to match".format(binary_minimum))
    return {"architecture": "arm64", "platform": platform, "minimum_ios": binary_minimum}


def signing_inputs(identity, profile_path, bundle_id, device_udid):
    if not identity and not profile_path:
        if device_udid:
            raise PackageError("--device-udid requires a signing identity and provisioning profile")
        return None
    if not identity or not profile_path:
        raise PackageError("provide both --signing-identity and --provisioning-profile")
    if identity == "-":
        raise PackageError("ad hoc signing cannot launch on a real iPhone")
    if not profile_path.is_file():
        raise PackageError("provisioning profile is not a file")
    profile = plistlib.loads(run(["security", "cms", "-D", "-i", str(profile_path)]))
    expiration = profile.get("ExpirationDate")
    if not isinstance(expiration, datetime.datetime):
        raise PackageError("profile has no expiration date")
    now = datetime.datetime.now(datetime.timezone.utc)
    if expiration.replace(tzinfo=datetime.timezone.utc) <= now:
        raise PackageError("provisioning profile has expired")
    if not profile.get("ProvisionedDevices") and not profile.get("ProvisionsAllDevices"):
        raise PackageError("profile does not allow direct device installation")
    if device_udid and not profile.get("ProvisionsAllDevices"):
        if device_udid not in profile.get("ProvisionedDevices", []):
            raise PackageError("the requested device is absent from the provisioning profile")
    allowed = profile.get("Entitlements", {})
    app_pattern = allowed.get("application-identifier", "")
    prefixes = profile.get("ApplicationIdentifierPrefix", [])
    if not prefixes or not app_pattern:
        raise PackageError("profile lacks an application identifier prefix")
    app_identifier = prefixes[0] + "." + bundle_id
    if not fnmatch.fnmatchcase(app_identifier, app_pattern):
        raise PackageError("profile's App ID does not authorize --bundle-id")
    team = allowed.get("com.apple.developer.team-identifier")
    if not team or team not in profile.get("TeamIdentifier", []):
        raise PackageError("profile has inconsistent team identifiers")
    identities = run(["security", "find-identity", "-v", "-p", "codesigning"]).decode()
    candidates = re.findall(r'\)\s+([0-9A-Fa-f]{40})\s+"([^"]+)"', identities)
    matching = [(digest.upper(), name) for digest, name in candidates
                if identity.upper() == digest.upper() or identity == name]
    if len(matching) != 1:
        raise PackageError("--signing-identity must exactly match one valid identity name or SHA-1")
    identity_hash = matching[0][0]
    authorized = {hashlib.sha1(cert).hexdigest().upper()
                  for cert in profile.get("DeveloperCertificates", [])}
    if identity_hash not in authorized:
        raise PackageError("signing identity is not authorized by the provisioning profile")
    entitlements = {
        "application-identifier": app_identifier,
        "com.apple.developer.team-identifier": team,
    }
    if "get-task-allow" in allowed:
        entitlements["get-task-allow"] = allowed["get-task-allow"]
    return {"identity": identity_hash, "entitlements": entitlements,
            "expiration": expiration.isoformat() + "Z", "team": team}


def copy_assets(source, destination):
    records = []

    def copy_directory(directory, target, ancestors):
        real = directory.resolve(strict=True)
        if real in ancestors:
            raise PackageError("asset symlink directory cycle at " + str(directory))
        target.mkdir()
        names = set()
        for entry in sorted(directory.iterdir()):
            folded = entry.name.casefold()
            if folded in names:
                raise PackageError("case-colliding asset name at " + str(entry))
            names.add(folded)
            copied = target / entry.name
            if entry.is_dir():
                copy_directory(entry, copied, ancestors + (real,))
            elif entry.is_file():
                # copyfile follows file symlinks; no source files are modified.
                shutil.copyfile(entry, copied)
                copied.chmod(0o644)
                records.append({"path": copied.relative_to(destination).as_posix(),
                                "bytes": copied.stat().st_size, "sha256": sha256(copied)})
            else:
                raise PackageError("asset is missing or not a regular file/directory: " + str(entry))

    copy_directory(source, destination, ())
    return records


def compile_app_icon(source, app, staging, minimum_ios, simulator):
    """Build the approved square artwork through Apple's asset catalog compiler."""
    properties = run(["sips", "-g", "pixelWidth", "-g", "pixelHeight", "-g", "hasAlpha",
                      str(source)]).decode()
    width = re.search(r"pixelWidth:\s+(\d+)", properties)
    height = re.search(r"pixelHeight:\s+(\d+)", properties)
    if not width or not height or width.group(1) != height.group(1) or int(width.group(1)) < 1024:
        raise PackageError("app icon must be a square image at least 1024 pixels across")
    if not re.search(r"hasAlpha:\s+no\b", properties):
        raise PackageError("app icon must be opaque, without an alpha channel")
    with tempfile.TemporaryDirectory(prefix="app-icon-", dir=staging) as temporary:
        catalog = Path(temporary) / "Branding.xcassets"
        icon_set = catalog / "AppIcon.appiconset"
        icon_set.mkdir(parents=True)
        (catalog / "Contents.json").write_text(json.dumps({"info": {"author": "xcode", "version": 1}}))
        images = []
        # Universal iPhone/iPad slots, including Settings, Spotlight, notification,
        # home-screen and marketing sizes. Duplicate pixel sizes share one PNG.
        slots = [("iphone", size, scale) for size in (20, 29, 40, 60) for scale in (2, 3)]
        slots += [("ipad", size, scale) for size in (20, 29, 40, 76) for scale in (1, 2)]
        slots += [("ipad", 83.5, 2), ("ios-marketing", 1024, 1)]
        for idiom, size, scale in slots:
            pixels = int(size * scale)
            filename = "AppIcon-{}.png".format(pixels)
            image = icon_set / filename
            if not image.exists():
                run(["sips", "-z", str(pixels), str(pixels), str(source), "--out", str(image)])
            images.append({"idiom": idiom, "size": "{0}x{0}".format(size),
                           "scale": "{}x".format(scale), "filename": filename})
        (icon_set / "Contents.json").write_text(json.dumps({"images": images,
                                                           "info": {"author": "xcode", "version": 1}}))
        partial_plist = Path(temporary) / "app-icon.plist"
        before = set(app.iterdir())
        # actool(1): merge the emitted partial plist so the icons in Assets.car and
        # loose fallback PNGs have Apple's matching CFBundleIcons declarations.
        run(["xcrun", "actool", "--compile", str(app), "--platform",
             "iphonesimulator" if simulator else "iphoneos", "--minimum-deployment-target", minimum_ios,
             "--target-device", "iphone", "--target-device", "ipad", "--app-icon", "AppIcon",
             "--standalone-icon-behavior", "all", "--output-partial-info-plist", str(partial_plist),
             "--output-format", "xml1", "--warnings", "--errors", str(catalog)])
        icon_plist = plistlib.loads(partial_plist.read_bytes())
        if not all(key in icon_plist for key in ("CFBundleIcons", "CFBundleIcons~ipad")):
            raise PackageError("actool did not produce both iPhone and iPad icon declarations")
        compiled = [{"path": path.name, "bytes": path.stat().st_size, "sha256": sha256(path)}
                    for path in sorted(set(app.iterdir()) - before) if path.is_file()]
    return icon_plist, {"source": str(source), "sha256": sha256(source),
                        "asset_name": "AppIcon", "compiled": compiled}


def info_plist(args):
    return {
        "CFBundleDevelopmentRegion": "zh-Hans",
        "CFBundleDisplayName": DISPLAY_NAME,
        "CFBundleExecutable": EXECUTABLE_NAME,
        "CFBundleIdentifier": args.bundle_id,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": DISPLAY_NAME,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": args.version,
        "CFBundleSupportedPlatforms": ["iPhoneSimulator" if args.simulator else "iPhoneOS"],
        "CFBundleVersion": args.build_number,
        "LSRequiresIPhoneOS": True,
        "MinimumOSVersion": args.minimum_ios,
        "UIDeviceFamily": [1, 2],
        "UIRequiredDeviceCapabilities": ["arm64", "metal"],
        "UILaunchScreen": {},
        "UIRequiresFullScreen": True,
        "UIStatusBarHidden": True,
        "UIViewControllerBasedStatusBarAppearance": False,
        "UIApplicationSupportsIndirectInputEvents": True,
        "UISupportedInterfaceOrientations": ["UIInterfaceOrientationLandscapeLeft",
                                              "UIInterfaceOrientationLandscapeRight"],
        "UISupportedInterfaceOrientations~ipad": ["UIInterfaceOrientationLandscapeLeft",
                                                   "UIInterfaceOrientationLandscapeRight"],
    }


def package(args):
    if not re.fullmatch(r"[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+", args.bundle_id):
        raise PackageError("--bundle-id must be a reverse-DNS bundle identifier")
    if version_tuple(args.minimum_ios) < (16, 0, 0):
        raise PackageError("the iOS mini app requires --minimum-ios 16.0 or newer")
    if not re.fullmatch(r"\d+(?:\.\d+){0,2}", args.build_number):
        raise PackageError("--build-number must contain one to three numeric components")
    if args.simulator and (args.provisioning_profile or args.device_udid or args.signing_identity):
        raise PackageError("--simulator uses ad hoc signing; omit profile, identity, and device UDID")
    platform = "IOSSIMULATOR" if args.simulator else "IOS"
    check_output(args.output, args.replace, platform)
    executable = args.executable.resolve(strict=True)
    asset_root = args.asset_root.resolve(strict=True)
    app_icon = args.app_icon.resolve(strict=True)
    subtitle = args.movie_subtitles.resolve(strict=True) if args.movie_subtitles else None
    output = args.output.resolve()
    if not executable.is_file() or not asset_root.is_dir() or not app_icon.is_file():
        raise PackageError("executable and app-icon must be files and asset-root must be a directory")
    if (contains(output, executable) or contains(output, asset_root) or contains(asset_root, output)
            or contains(output, app_icon) or (subtitle and contains(output, subtitle))):
        raise PackageError("output must be separate from the executable, icon and asset source tree")
    required = {"ra2.mix", "ra2md.mix", "language.mix", "langmd.mix", "gamemd.exe"}
    present = {path.name.casefold() for path in asset_root.iterdir() if path.is_file()}
    missing = sorted(required - present)
    if missing:
        raise PackageError("asset-root is missing retail files: " + ", ".join(missing))
    binary = inspect_binary(executable, args.minimum_ios, args.simulator)
    signing = signing_inputs(args.signing_identity, args.provisioning_profile,
                             args.bundle_id, args.device_udid)
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".vera20k-ios-", dir=str(output.parent)))
    try:
        app = staging / "Payload" / APP_NAME
        app.mkdir(parents=True)
        shutil.copyfile(executable, app / EXECUTABLE_NAME)
        (app / EXECUTABLE_NAME).chmod(0o755)
        icon_plist, icon_record = compile_app_icon(app_icon, app, staging, args.minimum_ios, args.simulator)
        with (app / "Info.plist").open("wb") as target:
            plistlib.dump({**info_plist(args), **icon_plist}, target, fmt=plistlib.FMT_BINARY)
        (app / "PkgInfo").write_bytes(b"APPL????")
        subtitle_record = None
        if subtitle:
            if not subtitle.is_file():
                raise PackageError("movie-subtitles must be a regular file")
            shutil.copyfile(subtitle, app / "subtitlemd.txt")
            attribution = []
            for name in ("creditsmd.txt", "README.upstream.md"):
                source = subtitle.parent / name
                if source.is_file():
                    shutil.copyfile(source, app / name)
                    attribution.append({"path": name, "sha256": sha256(source)})
            subtitle_record = {"source": str(subtitle), "sha256": sha256(subtitle),
                               "path": "subtitlemd.txt", "attribution": attribution}
        print("Copying local retail assets into the iOS app...", flush=True)
        # iOS uses a shallow bundle. A top-level directory literally named
        # Resources is reserved by the macOS bundle layout and confuses codesign.
        assets = copy_assets(asset_root, app / "retail-assets")
        if signing:
            shutil.copyfile(args.provisioning_profile, app / "embedded.mobileprovision")
            entitlements_path = staging / "signing-entitlements.plist"
            with entitlements_path.open("wb") as target:
                plistlib.dump(signing["entitlements"], target)
            # DER entitlements are needed for current iOS signatures. See Apple TN3125
            # and https://developer.apple.com/documentation/xcode/using-the-latest-code-signature-format
            run(["codesign", "--force", "--sign", signing["identity"], "--timestamp=none",
                 "--generate-entitlement-der", "--entitlements", str(entitlements_path), str(app)])
            run(["codesign", "--verify", "--strict", "--verbose=2", str(app)])
        elif args.simulator:
            run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(app)])
            run(["codesign", "--verify", "--strict", "--verbose=2", str(app)])
        ipa = staging / (DISPLAY_NAME + ("-ios-simulator.ipa" if args.simulator else "-ios.ipa"))
        signing_mode = "simulator-ad-hoc" if args.simulator else "provisioned" if signing else "unsigned"
        print("Writing {} IPA...".format(signing_mode), flush=True)
        with zipfile.ZipFile(ipa, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=1) as archive:
            for path in sorted((staging / "Payload").rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(staging).as_posix())
        manifest = {
            "owner": OWNER,
            "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "bundle_id": args.bundle_id,
            "display_name": DISPLAY_NAME,
            "app_name": APP_NAME,
            "version": args.version,
            "build_number": args.build_number,
            "app_icon": icon_record,
            "movie_subtitles": subtitle_record,
            "minimum_ios": args.minimum_ios,
            "signed": bool(signing) or args.simulator,
            "signing_mode": signing_mode,
            "signing_team": signing["team"] if signing else None,
            "profile_expiration": signing["expiration"] if signing else None,
            "executable_source": str(executable),
            "executable": {**binary, "bytes": (app / EXECUTABLE_NAME).stat().st_size,
                           "sha256": sha256(app / EXECUTABLE_NAME)},
            "asset_source": str(asset_root),
            "asset_count": len(assets),
            "asset_bytes": sum(item["bytes"] for item in assets),
            "assets": assets,
            "ipa_sha256": sha256(ipa),
        }
        (staging / MARKER).write_text(json.dumps(manifest, indent=2) + "\n")
        # Recheck ownership before replacing a previous generated package.
        check_output(args.output, args.replace, platform)
        if output.exists():
            shutil.rmtree(output)
        staging.rename(output)
        print("App: " + str(output / "Payload" / APP_NAME))
        print("IPA: " + str(output / ipa.name))
        status = ("simulator only; launch still needs validation" if args.simulator else
                  "signed; device launch still needs validation" if signing else
                  "unsigned; obtain a device profile and sign before installing")
        print("Status: " + status)
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", required=True, type=absolute_path,
                        help="absolute Rust binary path (device target unless --simulator)")
    parser.add_argument("--asset-root", required=True, type=absolute_path,
                        help="absolute path to local retail game files; symlinks are dereferenced")
    parser.add_argument("--output", required=True, type=absolute_path,
                        help="new or empty output directory")
    parser.add_argument("--app-icon", type=absolute_path, default=APP_ICON,
                        help="approved opaque square PNG (defaults to art/branding/wannafire-app-icon.png)")
    parser.add_argument("--movie-subtitles", type=absolute_path,
                        help="optional local YR subtitlemd.txt, with sibling credits/source attribution")
    parser.add_argument("--bundle-id", default="org.yuriplanet.vera20k.ios.mini")
    parser.add_argument("--minimum-ios", type=version, default="16.0")
    parser.add_argument("--version", type=version, default="1.0")
    parser.add_argument("--build-number", default="1")
    parser.add_argument("--simulator", action="store_true",
                        help="require an arm64 iOS Simulator binary and sign ad hoc; use a separate output")
    parser.add_argument("--signing-identity", help="exact installed identity name or SHA-1 hash")
    parser.add_argument("--provisioning-profile", type=absolute_path,
                        help="current iOS development/ad hoc provisioning profile")
    parser.add_argument("--device-udid", help="optionally require the profile to include this device")
    parser.add_argument("--replace", action="store_true",
                        help="replace a package directory previously generated by this tool")
    args = parser.parse_args()
    try:
        package(args)
    except (PackageError, OSError, ValueError, plistlib.InvalidFileException) as error:
        parser.exit(1, "error: {}\n".format(error))


if __name__ == "__main__":
    main()
