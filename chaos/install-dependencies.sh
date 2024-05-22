#!/bin/sh

echo "Checking OS and installing dependencies..."

if [ "$(uname)" = "Darwin" ]; then
    echo "Installing dependencies for macOS..."
    if ! [ -x "$(command -v kind)" ]; then
        brew install kind
    fi
    if ! [ -x "$(command -v kubectl)" ]; then
        brew install kubectl
    fi
else
    echo "Installing dependencies for Linux..."
    if ! [ -x "$(command -v kind)" ]; then
        curl -Lo ./kind https://kind.sigs.k8s.io/dl/v0.11.1/kind-linux-amd64
        chmod +x ./kind
        sudo mv ./kind /usr/local/bin/kind
    fi
    if ! [ -x "$(command -v kubectl)" ]; then
        curl -LO "https://storage.googleapis.com/kubernetes-release/release/$(curl -s https://storage.googleapis.com/kubernetes-release/release/stable.txt)/bin/linux/amd64/kubectl"
        chmod +x ./kubectl
        sudo mv ./kubectl /usr/local/bin/kubectl
    fi
fi

echo "Dependencies installed"
