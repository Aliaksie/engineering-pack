# Project evidence local bundle

Target: x86_64-pc-windows-msvc. Requires Git on PATH; Node and Rust are not required.

Run from this directory:

```sh
native-bin/craft.exe project-evidence version --enable
native-bin/craft.exe project-evidence refresh --enable --root /absolute/repository
native-bin/craft.exe project-evidence query --enable --root /absolute/repository --value symbol --verify
```

Move the whole directory together. Omit --enable to disable native access. No hooks or host configuration are installed. Keep the previous bundle directory for rollback; source evidence remains in each selected repository's private .craft state. Replacing a bundle does not migrate that state. Other targets are unverified. See dependencies.json and notices/ for dependency notices.
