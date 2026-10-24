curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.0 -c clippy -c rustfmt
echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"

