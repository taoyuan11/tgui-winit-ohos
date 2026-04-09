# `ohos-shell`

Minimal Stage Model shell for validating `tgui-winit-ohos` with an ArkUI `XComponent`.

Before building the shell, build the Rust demo library for the desired target:

```powershell
cargo build --manifest-path ..\ohos-smoke\Cargo.toml --target x86_64-unknown-linux-ohos
```

For a device build, replace the target with `aarch64-unknown-linux-ohos`.

Then run:

```powershell
"D:\Apps\code\DevEco Studio\tools\ohpm\bin\ohpm.bat" install
```

from this directory and build the `entry` module in DevEco Studio or with Hvigor.
