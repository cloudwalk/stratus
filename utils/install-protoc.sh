#!/bin/bash

# Define the protoc version and target architecture
PROTOC_VERSION="3.20.2"
ARCH="osx-aarch_64"

# Download protoc
echo "Downloading protoc ${PROTOC_VERSION} for ${ARCH}..."
curl -LO "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/protoc-${PROTOC_VERSION}-${ARCH}.zip"

# Unzip the file
echo "Unpacking protoc..."
unzip "protoc-${PROTOC_VERSION}-${ARCH}.zip" -d protoc

# Move protoc binary to /usr/local/bin
echo "Installing protoc..."
sudo mv protoc/bin/protoc /usr/local/bin/

# Optionally, move the include files
echo "Installing include files..."
sudo mv protoc/include/* /usr/local/include/

# Clean up the downloaded and unpacked files
echo "Cleaning up..."
rm -rf protoc
rm "protoc-${PROTOC_VERSION}-${ARCH}.zip"

# Verify the installation
echo "Verifying the installation..."
protoc --version
