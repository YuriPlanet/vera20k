# SignPath onboarding

`windows-signing-check.yml` is a manual, non-publishing diagnostic for the
SignPath Foundation onboarding review. It builds the Windows game on a GitHub
runner, signs its executable and Inno uninstaller, rebuilds the installer, then
signs and installs that final installer. Its output uses the supplied **test
certificate** and is not a public download.

The normal Release workflow still produces its existing Windows installer until
SignPath has reviewed the test and supplied a production certificate. Never put
the test certificate or test-signed installers into public releases, and never
install its test root on players' computers.

## Account and project

Use the invited **VERA20k [OSS]** organization and its existing **CI builds**
user. Do not configure a separate self-service workspace. Enable MFA for the
interactive maintainer account.

Use the prepared project `vera20k`, its supplied test certificate, and the
predefined trusted build system **GitHub.com**. Its repository URL is
`https://github.com/YuriPlanet/vera20k`. Add the two artifact configurations in
this directory:

- `payload`: `signpath-payload.xml`
- `installer`: `signpath-installer.xml`

Use the prepared `test-signing` policy with the provided self-signed certificate.
The policy permits only builds from `main` using exactly
`.github/workflows/windows-signing-check.yml`, and requires maintainer approval.
The CI user has only submission permission for this project/policy; the
maintainer approves the requests. Do not approve them on the maintainer's behalf.

The diagnostic pins SignPath's official v2.3 action and its classic connector,
which accepts the job's temporary, read-only GitHub token for basic origin
verification. Keep the repository, branch, workflow and approval restrictions
enabled. Review any connector-generation upgrade before changing this pin:
the v3 connector requested the SignPath GitHub App for this organization, and
that app requests repository and organization Administration write permissions.
Such an installation requires a separate access decision even when its selected
repository is limited to VERA20k. See the [GitHub integration documentation](https://docs.signpath.io/trusted-build-systems/github)
and [connector history](https://docs.signpath.io/changelog/).

The OSS subscription supplies its Authenticode watermark automatically. Keep
the XML signing instructions as `<authenticode-sign />`; do not set `description`
or `description-url`, which would override that enforced watermark.

The metadata restrictions require the product name `VERA20k` and the build's
product version. Inno 7.1.0 leaves its uninstaller and setup resources padded with
spaces: ProductName occupies 60 characters and ProductVersion occupies 50.
The Inno XML matchers include that exact padding after `${version}`; the game's
matchers remain unpadded. Keep padding in the XML: SignPath strips trailing
spaces from submitted parameter values. Before upload, `metadata` checks that
both Inno matchers have the same ASCII-space suffix and that the expanded
version occupies exactly 50 characters. If a version changes width (for example,
`0.2.5` to `0.2.10`), adjust the suffix in both XML files and their SignPath portal
configurations before retrying. See Inno's
[resource writer](https://github.com/jrsoftware/issrc/blob/is-7_1_0/Projects/Src/Compiler.ExeUpdateFunc.pas)
and [version resources](https://github.com/jrsoftware/issrc/blob/is-7_1_0/Projects/Src/Setup.version.rc).
The installer's visible name (`AppName`) is not matched. ZIP archives
contain the files directly at their root, as GitHub's artifact upload creates
them. See [SignPath's GitHub integration](https://docs.signpath.io/trusted-build-systems/github)
and [artifact configuration reference](https://docs.signpath.io/artifact-configuration/reference).

## Protected GitHub environment

Use `windows-signing`, restricted to the exact `main` branch and with the
maintainer as a required reviewer. Do not approve runs on the maintainer's behalf.

Environment secret:

- `SIGNPATH_API_TOKEN`: the CI user's submitter token, never an administrator token.

Environment variables:

- `SIGNPATH_ORGANIZATION_ID`: the invited OSS organization UUID.
- `SIGNPATH_PROJECT_SLUG`: `vera20k`.
- `SIGNPATH_TEST_POLICY_SLUG`: `test-signing`.
- `SIGNPATH_TEST_CERTIFICATE_BASE64`: base64 of the public test certificate in DER format.
- `SIGNPATH_TEST_CERTIFICATE_SHA256`: SHA256 fingerprint of those DER bytes.

Keep the token only in the private deployment store and this environment. Never
paste it into a workflow argument, log, issue, or pull request. The public test
certificate is not a secret. The diagnostic checks its fingerprint before
trusting it on the temporary runner, and verifies every signature against that
same fingerprint.

The runner imports that public test certificate into `LocalMachine\Root`,
with its exact SHA256 checked before and after import. A `CurrentUser\Root`
import can wait for an interactive trust prompt. This trust is confined to the
disposable test VM; it is never installed on a player's machine.

## Installer signing order

[`signing.py`](signing.py) owns the compiler commands for both ordinary packaging
and the diagnostic:

1. `prepare` copies the package and asks Inno 7.1.0 to generate its uninstaller.
   It accepts only the compiler's specific signature-needed result; unrelated
   compiler failures still fail the build. A manifest records the original
   files and Inno cache name.
2. SignPath signs `vera20k.exe` and `uninstaller.exe` in that ZIP.
3. `package` compares the returned files and manifest with the retained original
   payload, verifies both signatures, restores the Inno cache name, and compiles
   with the signed uninstaller.
4. SignPath signs the final `*-setup.exe` in a second ZIP.
5. The diagnostic requires the expected setup filename, compares its executable
   content with the original unsigned installer, and verifies its signature.
   It then installs against the existing synthetic game fixture, verifies the
   installed game and uninstaller, and uninstalls it.

Inno documents why the signed uninstaller also matters for temporary setup
self-copies: [SignedUninstaller](https://jrsoftware.org/ishelp/topic_setup_signeduninstaller.htm).

For a runner or Inno compiler regression, dispatch with `packaging_only`
enabled. It checks noninteractive trust with a throwaway certificate, then
ordinary packaging and uninstaller preparation with inert fixture bytes.
It does not compile the game, access a signing token, or send fixtures to SignPath.

To retry signing an existing game build, set `reuse_run_id` to its original
compilation run's ID. The helper requires a completed main-branch run of this workflow with a
successful build, a retained unsigned payload, and a source commit ancestral
to the current run. It validates the original payload and preserves its name,
version and build time. The log records the original source commit: this is a
test of that earlier build, not a fresh build of the current source. The new
signing job still requires maintainer approval. Leave this input empty for a
fresh game build.

## Production follow-up

After a successful test, request SignPath's configuration review and production
certificate. Public signing still requires the public code-signing/privacy
policies and any remaining Foundation requirements. Only then wire production
signing and signature verification into Release's publication and promotion
gates; the production path must not trust the test certificate.
