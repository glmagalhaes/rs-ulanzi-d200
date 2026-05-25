#!/bin/bash
set -e  # exit on any error

# ---------- Configuration ----------
BINARY_NAME="rs-ulanzi-d200-linux"
MANIFEST_SRC="src/manifest.json"
ASSETS_SRC="src/assets"
CONFIG_YAML="config.yaml"
PLUGIN_FOLDER="com.glmagalhaes.ulanzi.d200.sdPlugin"

# ---------- Usage ----------
if [ $# -ne 1 ]; then
    echo "Usage: $0 {debug|release}"
    exit 1
fi

MODE="$1"

# ---------- Set paths based on mode ----------
if [ "$MODE" = "debug" ]; then
    BINARY_PATH="target/debug/$BINARY_NAME"
    ZIP_NAME="rs-ulanzi-d200-debug.zip"
elif [ "$MODE" = "release" ]; then
    BINARY_PATH="target/release/$BINARY_NAME"
    ZIP_NAME="com.glmagalhaes.ulanzi.d200.zip"
else
    echo "Invalid mode: $MODE (use 'debug' or 'release')"
    exit 1
fi

# ---------- Build ----------
if [ "$MODE" = "debug" ]; then
    cargo build  
elif [ "$MODE" = "release" ]; then
    cargo build --release  
fi

# ---------- Check that the binary exists ----------
if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: Binary not found at $BINARY_PATH"
    echo "Make sure you have built the project: cargo build --$MODE"
    exit 1
fi

# ---------- Check required source files ----------
if [ ! -f "$MANIFEST_SRC" ]; then
    echo "Error: $MANIFEST_SRC not found"
    exit 1
fi

if [ ! -d "$ASSETS_SRC" ]; then
    echo "Error: $ASSETS_SRC directory not found"
    exit 1
fi

if [ ! -f "$CONFIG_YAML" ]; then
    echo "Error: $CONFIG_YAML directory not found"
    exit 1
fi


# ---------- Create temporary packaging directory ----------
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT  # clean up on exit

# ---------- Create the required parent folder ----------
mkdir -p "$TMP_DIR/$PLUGIN_FOLDER"

# ---------- Copy files into the plugin folder ----------
cp "$BINARY_PATH" "$TMP_DIR/$PLUGIN_FOLDER/"
cp "$MANIFEST_SRC" "$TMP_DIR/$PLUGIN_FOLDER/"
cp "$CONFIG_YAML" "$TMP_DIR/$PLUGIN_FOLDER/"
cp -r "$ASSETS_SRC" "$TMP_DIR/$PLUGIN_FOLDER/"

# ---------- Create zip ----------
rm -f "$ZIP_NAME"
cd "$TMP_DIR"
zip -r "$OLDPWD/$ZIP_NAME" "$PLUGIN_FOLDER" > /dev/null
cd - > /dev/null

echo "Successfully created $ZIP_NAME"
echo "Zip contains the top-level folder: $PLUGIN_FOLDER/"