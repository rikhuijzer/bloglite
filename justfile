shebang := '''
    /usr/bin/env bash
    set -euo pipefail
'''

alias r := release
alias s := serve

default:
    just --list

tag:
    #!{{shebang}}

    METADATA="$(cargo metadata --format-version=1 --no-deps)"
    VERSION="$(echo $METADATA | jq -r '.packages[0].version')"
    echo "VERSION: $VERSION"
    TAGNAME="v$VERSION"
    echo "TAGNAME: $TAGNAME"

    echo "Existing tags:"
    git tag
    echo ""
    read -p "Are you sure you want to tag $TAGNAME? Type YES to continue. " REPLY
    if [[ $REPLY == "YES" ]]; then
        echo ""
        git tag -a $TAGNAME -m "Release $VERSION"
        git push origin $TAGNAME
        exit 0
    else
        echo ""
        echo "Did not receive YES, aborting"
        exit 1
    fi

# Build release and copy files to public/
#
# The copy might speed up Docker builds due to fewer files in the context.
release: static-assets
    #!{{shebang}}

    rm -rf public
    mkdir -p public

    ARCH="$(uname -m)"
    echo "ARCH: $ARCH"
    OS="$(uname -s)"
    echo "OS: $OS"

    if [[ $OS == "Linux" && $ARCH == "x86_64" ]]; then
        TARGET="x86_64-unknown-linux-musl"
    elif [[ $OS == "Linux" && ( $ARCH == "aarch64" || $ARCH == "arm64" ) ]]; then
        TARGET="aarch64-unknown-linux-musl"
    else
        echo "Container releases require Linux on amd64 or arm64" >&2
        exit 1
    fi

    # Both CI runners build natively. Use musl for Rust linking and native dependencies.
    TARGET_ENV="${TARGET//-/_}"
    export "CC_${TARGET_ENV}=musl-gcc"
    export "CARGO_TARGET_${TARGET_ENV^^}_LINKER=musl-gcc"
    cargo build -p bloglite --release --locked --target="$TARGET"
    cp --verbose "target/$TARGET/release/bloglite" "public/"
    mkdir -p public/licenses
    cp bloglite/src/static/vendor/mathml-LICENSE bloglite/src/static/vendor/LibertinusMath-OFL.txt public/licenses/

    # The static runtime has no dynamic loader or shared libraries.
    readelf --program-headers public/bloglite > public/program-headers.txt
    readelf --dynamic public/bloglite > public/dynamic.txt
    if grep -q 'INTERP' public/program-headers.txt || grep -q '(NEEDED)' public/dynamic.txt; then
        echo "Release binary must be statically linked" >&2
        exit 1
    fi
    rm public/program-headers.txt public/dynamic.txt

    cat > public/Dockerfile << EOF
    FROM gcr.io/distroless/static-debian13
    ENV BLOGLITE_PRODUCTION="true"
    COPY bloglite /
    COPY licenses /licenses
    CMD ["/bloglite", "serve"]
    EOF

    cat public/Dockerfile

    # To avoid accidentally editing the files in public manually.
    chmod 444 public/Dockerfile

serve: static-assets
    #!{{shebang}}

    cargo watch -x "run -- serve --password=test-password"

# Fetch and verify third-party math assets (cached locally).
static-assets:
    #!{{shebang}}

    # math-core v0.7.0 assets; checksums pin the exact bytes.
    asset_dir="bloglite/src/static/vendor"
    mkdir -p "$asset_dir"
    asset_tmp="$(mktemp -d "$asset_dir/.download.XXXXXX")"
    trap 'rm -rf "$asset_tmp"' EXIT

    checksum() {
        if command -v sha256sum >/dev/null 2>&1; then
            sha256sum "$1" | cut -d ' ' -f 1
        else
            shasum -a 256 "$1" | cut -d ' ' -f 1
        fi
    }

    while read -r expected name url; do
        if [[ -f "$asset_dir/$name" ]] && [[ "$(checksum "$asset_dir/$name")" == "$expected" ]]; then
            continue
        fi
        printf 'Downloading %s\n' "$name"
        curl --fail --location --retry 2 --connect-timeout 15 --max-time 120 "$url" -o "$asset_tmp/$name"
        if [[ "$(checksum "$asset_tmp/$name")" != "$expected" ]]; then
            printf 'Checksum mismatch for %s\n' "$name" >&2
            exit 1
        fi
        mv "$asset_tmp/$name" "$asset_dir/$name"
    done <<'ASSETS'
    82338364c78cdb5c14301bb44efd3e25dfb4b236217d20c7df833e44888b8a3f mathml.css https://raw.githubusercontent.com/tmke8/math-core/v0.7.0/css/mathmlfixes.css
    4522db391501c9fde3553b377a9cec8bdd05eb80dd0bb3df51d9e44c3e4c8f2f mathml-LICENSE https://raw.githubusercontent.com/tmke8/math-core/v0.7.0/LICENSE
    6510228cbca8ce1485d7de9078830c5096d4c35fcce3fea8b923caf6b3f091d9 LibertinusMath-Regular.woff2 https://raw.githubusercontent.com/tmke8/math-core/v0.7.0/playground/fonts/LibertinusMath-Regular.woff2
    9aeecc8107c489ec1ec0068b0313e531a760edf3493705b32ab8ab8215a8794e LibertinusMath-OFL.txt https://raw.githubusercontent.com/tmke8/math-core-fonts/main/LibertinusMath/OFL.txt
    ASSETS

# Test after preparing the assets required by include_str!/include_bytes!.
test: static-assets
    cargo test --workspace --locked
