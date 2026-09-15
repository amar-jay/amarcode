# Desktop updater release setup

Amarcode uses Tauri's signed updater artifacts and GitHub Releases. The public
key is embedded in `crates/application/src-tauri/tauri.conf.json`; the matching
private key must never be committed.

## One-time GitHub secret setup

The generated private key is stored locally at:

```text
/home/manan/.config/amarcode/app-updater.key
```

Upload it to this repository with GitHub CLI:

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY < /home/manan/.config/amarcode/app-updater.key
```

This key was generated without a password, so
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is optional and may be left unset. If the
key is replaced with a password-protected key, upload that password too:

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

`GITHUB_TOKEN` is created automatically for each Actions run; do not create a
repository secret for it.

For a signed local build, provide the key by path:

```sh
export TAURI_SIGNING_PRIVATE_KEY_PATH=/home/manan/.config/amarcode/app-updater.key
bun run app:build
```

## Publishing

1. Commit and push to `main`. A package version bump is not required.
2. Run the **Release desktop app** workflow.

The workflow overwrites the rolling GitHub Release tagged `latest` (name:
`amarcode desktop`) with installers, signed updater archives, signatures, and
`latest.json`. Installed applications treat a new artifact signature as an
update even when the package version stays `0.1.0`.

Back up the private key securely. Losing it prevents existing installations
from accepting future updates. Rotating only the public key in a new release
does not update installations that still trust the old key.
