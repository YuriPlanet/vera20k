#!/usr/bin/env python3
"""Generate a tiny Xcode app project for automatic development provisioning.

Build this project with Xcode to obtain a current embedded.mobileprovision for the
real Rust app's bundle ID. Do not install the stub on the phone. This script only
writes a new local project; xcodebuild performs the account/device operations.
"""

import argparse
import json
from pathlib import Path
import re
import shlex


PROJECT = """// !$*UTF8*$!
{
  archiveVersion = 1;
  classes = {};
  objectVersion = 60;
  objects = {
    A00000000000000000000001 = {isa = PBXBuildFile; fileRef = A00000000000000000000002; };
    A00000000000000000000002 = {isa = PBXFileReference; lastKnownFileType = sourcecode.c.objc; path = main.m; sourceTree = "<group>"; };
    A00000000000000000000003 = {isa = PBXFileReference; explicitFileType = wrapper.application; includeInIndex = 0; path = VERA20kProvisioning.app; sourceTree = BUILT_PRODUCTS_DIR; };
    A00000000000000000000004 = {isa = PBXGroup; children = (A00000000000000000000002, A00000000000000000000005); sourceTree = "<group>"; };
    A00000000000000000000005 = {isa = PBXGroup; children = (A00000000000000000000003); name = Products; sourceTree = "<group>"; };
    A00000000000000000000006 = {isa = PBXSourcesBuildPhase; buildActionMask = 2147483647; files = (A00000000000000000000001); runOnlyForDeploymentPostprocessing = 0; };
    A00000000000000000000007 = {isa = PBXFrameworksBuildPhase; buildActionMask = 2147483647; files = (); runOnlyForDeploymentPostprocessing = 0; };
    A00000000000000000000008 = {isa = PBXResourcesBuildPhase; buildActionMask = 2147483647; files = (); runOnlyForDeploymentPostprocessing = 0; };
    A00000000000000000000009 = {
      isa = PBXNativeTarget;
      buildConfigurationList = A0000000000000000000000A;
      buildPhases = (A00000000000000000000006, A00000000000000000000007, A00000000000000000000008);
      buildRules = ();
      dependencies = ();
      name = VERA20kProvisioning;
      productName = VERA20kProvisioning;
      productReference = A00000000000000000000003;
      productType = "com.apple.product-type.application";
    };
    A0000000000000000000000B = {
      isa = PBXProject;
      attributes = { BuildIndependentTargetsInParallel = YES; LastUpgradeCheck = 1600; TargetAttributes = { A00000000000000000000009 = { CreatedOnToolsVersion = 16.0; DevelopmentTeam = @TEAM@; }; }; };
      buildConfigurationList = A0000000000000000000000C;
      compatibilityVersion = "Xcode 15.0";
      developmentRegion = en;
      hasScannedForEncodings = 0;
      knownRegions = (en, Base);
      mainGroup = A00000000000000000000004;
      productRefGroup = A00000000000000000000005;
      projectDirPath = "";
      projectRoot = "";
      targets = (A00000000000000000000009);
    };
    A0000000000000000000000D = {
      isa = XCBuildConfiguration;
      buildSettings = {
        ALWAYS_SEARCH_USER_PATHS = NO;
        CLANG_ENABLE_MODULES = YES;
        CLANG_ENABLE_OBJC_ARC = YES;
        CODE_SIGN_STYLE = Automatic;
        CODE_SIGN_IDENTITY = "Apple Development";
        CURRENT_PROJECT_VERSION = 1;
        DEVELOPMENT_TEAM = @TEAM@;
        GENERATE_INFOPLIST_FILE = YES;
        INFOPLIST_KEY_CFBundleDisplayName = "VERA20k Provisioning Only";
        INFOPLIST_KEY_LSRequiresIPhoneOS = YES;
        INFOPLIST_KEY_UILaunchScreen_Generation = YES;
        INFOPLIST_KEY_UIRequiresFullScreen = YES;
        INFOPLIST_KEY_UISupportedInterfaceOrientations = "UIInterfaceOrientationLandscapeLeft UIInterfaceOrientationLandscapeRight";
        IPHONEOS_DEPLOYMENT_TARGET = 16.0;
        LD_RUNPATH_SEARCH_PATHS = "$(inherited) @executable_path/Frameworks";
        MARKETING_VERSION = 0.1.0;
        PRODUCT_BUNDLE_IDENTIFIER = @BUNDLE@;
        PRODUCT_NAME = "$(TARGET_NAME)";
        SDKROOT = iphoneos;
        SUPPORTED_PLATFORMS = iphoneos;
        SUPPORTS_MACCATALYST = NO;
        TARGETED_DEVICE_FAMILY = "1,2";
      };
      name = Debug;
    };
    A0000000000000000000000E = {isa = XCBuildConfiguration; buildSettings = { CLANG_ENABLE_MODULES = YES; SDKROOT = iphoneos; }; name = Debug; };
    A0000000000000000000000A = {isa = XCConfigurationList; buildConfigurations = (A0000000000000000000000D); defaultConfigurationIsVisible = 0; defaultConfigurationName = Debug; };
    A0000000000000000000000C = {isa = XCConfigurationList; buildConfigurations = (A0000000000000000000000E); defaultConfigurationIsVisible = 0; defaultConfigurationName = Debug; };
  };
  rootObject = A0000000000000000000000B;
}
"""

MAIN = """// Provisioning only: never install this placeholder instead of the Rust app.
#import <UIKit/UIKit.h>

@interface ProvisioningDelegate : UIResponder <UIApplicationDelegate>
@property (strong, nonatomic) UIWindow *window;
@end

@implementation ProvisioningDelegate
- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)options {
    self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];
    self.window.rootViewController = [UIViewController new];
    self.window.rootViewController.view.backgroundColor = UIColor.blackColor;
    [self.window makeKeyAndVisible];
    return YES;
}
@end

int main(int argc, char *argv[]) {
    @autoreleasepool {
        return UIApplicationMain(argc, argv, nil, NSStringFromClass(ProvisioningDelegate.class));
    }
}
"""

SCHEME = """<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion="1600" version="1.3">
  <BuildAction parallelizeBuildables="YES" buildImplicitDependencies="YES">
    <BuildActionEntries>
      <BuildActionEntry buildForTesting="NO" buildForRunning="YES" buildForProfiling="NO" buildForArchiving="NO" buildForAnalyzing="YES">
        <BuildableReference BuildableIdentifier="primary" BlueprintIdentifier="A00000000000000000000009" BuildableName="VERA20kProvisioning.app" BlueprintName="VERA20kProvisioning" ReferencedContainer="container:VERA20kProvisioning.xcodeproj"/>
      </BuildActionEntry>
    </BuildActionEntries>
  </BuildAction>
</Scheme>
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="new or empty absolute project directory")
    parser.add_argument("--team", required=True, help="Apple Developer team ID")
    parser.add_argument("--bundle-id", default="org.yuriplanet.vera20k.ios.mini")
    parser.add_argument("--device-id", help="connected iPhone UDID for the printed build command")
    args = parser.parse_args()
    output = args.output.expanduser()
    if not output.is_absolute() or output.is_symlink():
        parser.error("--output must be an absolute path, not a symlink")
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        parser.error("--output must be new or empty; choose a new directory for another project")
    if not re.fullmatch(r"[A-Z0-9]{10}", args.team):
        parser.error("--team must contain 10 uppercase letters/digits")
    if not re.fullmatch(r"[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+", args.bundle_id):
        parser.error("--bundle-id must be a reverse-DNS identifier")
    output = output.resolve()
    project = output / "VERA20kProvisioning.xcodeproj"
    schemes = project / "xcshareddata" / "xcschemes"
    schemes.mkdir(parents=True)
    (project / "project.pbxproj").write_text(PROJECT.replace("@TEAM@", args.team).replace("@BUNDLE@", args.bundle_id))
    (schemes / "VERA20kProvisioning.xcscheme").write_text(SCHEME)
    (output / "main.m").write_text(MAIN)
    (output / "provisioning-project.json").write_text(json.dumps({"purpose": "development provisioning only",
                                                               "team": args.team, "bundle_id": args.bundle_id}, indent=2) + "\n")
    command = ["xcodebuild", "-project", str(project), "-scheme", "VERA20kProvisioning",
               "-configuration", "Debug", "-sdk", "iphoneos", "-destination",
               "id=" + (args.device_id or "CONNECTED_DEVICE_UDID"), "-derivedDataPath",
               str(output / "DerivedData"), "-allowProvisioningUpdates",
               "-allowProvisioningDeviceRegistration", "build"]
    print("Generated: " + str(project))
    print("Build to obtain a profile; do not install this placeholder:")
    print(shlex.join(command))
    print("Profile: " + str(output / "DerivedData/Build/Products/Debug-iphoneos/VERA20kProvisioning.app/embedded.mobileprovision"))


if __name__ == "__main__":
    main()
