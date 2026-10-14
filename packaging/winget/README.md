# Kabuk on winget

The manifests under `manifests/` describe `Milvasoft.Kabuk` for the Windows Package Manager. The installer is the `kabuk-setup.exe` of the GitHub release `v<version>`; it installs for the current user without administrator rights and takes `/S` to install silently.

## Releasing a version

1. `powershell -ExecutionPolicy Bypass -File tools/package.ps1` builds `dist\kabuk-setup.exe` and writes its SHA-256 into this version's installer manifest.
2. Create the GitHub release `v<version>` and upload `dist\kabuk-setup.exe`.
3. Check the manifests: `winget validate --manifest packaging\winget\manifests\m\Milvasoft\Kabuk\<version>`.
4. Try them: `winget install --manifest packaging\winget\manifests\m\Milvasoft\Kabuk\<version>` (needs `winget settings --enable LocalManifestFiles` once, as administrator).
5. Submit them to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) (for example with `wingetcreate submit`). Once merged, anyone can run `winget install Milvasoft.Kabuk`.

For a new version, copy the folder to the new version number and update `PackageVersion`, the release URL and the notes link.
