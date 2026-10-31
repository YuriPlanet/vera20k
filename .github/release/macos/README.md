# Mac signing in the Release workflow

Dev builds run from `main`. The build job creates an ad-hoc-signed staging image;
the separate `macos-sign` job signs its app with Developer ID, notarizes its final
disk image, staples the ticket and verifies it. Updater tests and publication use
only that final image. Stable promotion reuses its exact bytes.

The `macos-signing` GitHub environment must allow **only the exact `main` branch**
through its deployment branch policy. A condition inside workflow YAML is not a
replacement for this server-side restriction. No required reviewer is needed for
automatic nightlies. Do not add Apple secrets to repository scope or the build job.

The environment holds:

- `MACOS_SIGNING_KEYCHAIN`: base64 of a dedicated, encrypted `.keychain-db` with
  only the Developer ID Application identity, its intermediate certificate and
  a `notarytool` profile named `VERA20k-notary`.
- `MACOS_SIGNING_KEYCHAIN_PASSWORD`: that keychain's password.
- Variables `MACOS_SIGNING_IDENTITY` (the certificate's SHA-1 identifier) and
  `APPLE_TEAM_ID` (its developer team).

The script unlocks the temporary keychain through Security.framework so the
password never enters command arguments. Its subprocesses do not inherit either
secret. It restores the runner's keychain search list and deletes the temporary
keychain even on failure; no application code runs while it is loaded.

Apple documents transferring file-based keychains between Macs:
<https://support.apple.com/guide/keychain-access/kyca1121/mac>.
GitHub documents environment branch protection:
<https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments>.

## A submission that takes longer than the job

The job waits up to 30 minutes. A timeout or rejection stops publication and saves
the signed image and its submission/status JSON in the `macos-notarization`
workflow artifact. No keychain or password is included. Resume the recorded
submission instead of repeatedly uploading the same image:

```sh
xcrun notarytool wait <submission-id> --keychain-profile VERA20k-notary --keychain <private-keychain> --timeout 48h
xcrun stapler staple <signed-image.dmg>
xcrun stapler validate <signed-image.dmg>
spctl --assess --type open --context context:primary-signature --verbose=4 <signed-image.dmg>
```

Only use the last three commands after Apple reports `Accepted`. The first
notarization for a new developer can take much longer than routine submissions.
