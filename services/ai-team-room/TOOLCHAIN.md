# AI Team Room service toolchain

The normative development and validation toolchain for this service is pinned to:

- Rust toolchain: `1.100.0`
- `rustc`: `1.100.0`
- `cargo`: `1.100.0`
- `rustfmt`: `1.11.0`
- Channel: `stable`, selected by the exact numeric toolchain version `1.100.0`

The version is intentionally numeric. Do not replace it with the moving `stable` channel. The upstream `rustfmt --version` output currently includes the suffix `-stable`; this is the release-channel label for rustfmt 1.11.0, not a different version. The adjacent `rust-toolchain.toml` is the source of truth for local rustup selection and installs the `rustfmt` component.

Run service commands from this directory so rustup discovers the service-local toolchain file:

```bash
rustc --version
cargo --version
rustfmt --version
cargo fmt --manifest-path Cargo.toml -- --check
cargo check --manifest-path Cargo.toml
```

CI must install the same exact toolchain version and the `rustfmt` component. The dedicated Phone-02B toolchain workflow validates the observed versions, formatter check, and compilation independently of the general repository workflows.

The service Docker builder currently uses `rust:1.86-bookworm`. That image is a separate build-environment pin and is not silently changed by this policy. Docker compatibility and any image upgrade require a separate validation decision.
