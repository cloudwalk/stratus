#!/bin/bash

set -e

echo "Checking if Kind cluster exists..."
if ! kind get clusters | grep -q local-testing; then
    echo "Setting up Kind cluster..."
    kind create cluster --name local-testing
    kind get kubeconfig --name local-testing > kubeconfig.yaml
else
    echo "Kind cluster already exists."
fi

echo "Configuring kubectl to use Kind cluster..."
export KUBECONFIG=$(pwd)/kubeconfig.yaml

mkdir -p ~/.docker/cli-plugins
curl -L https://github.com/docker/buildx/releases/download/v0.8.2/buildx-v0.8.2.darwin-amd64 -o ~/.docker/cli-plugins/docker-buildx
chmod +x ~/.docker/cli-plugins/docker-buildx

echo "Checking if Docker image is already built..."
if ! docker images | grep -q local/run_with_importer_cached; then
    echo "Building Docker image..."
    docker buildx inspect mybuilder > /dev/null 2>&1 || docker buildx create --name mybuilder --use
    DOCKER_BUILDKIT=1 docker buildx build -t local/run_with_importer_cached -f ./docker/Dockerfile.run_with_importer_cached .
else
    echo "Docker image already built."
fi

echo "Loading Docker image into Kind..."
kind load docker-image local/run_with_importer_cached --name local-testing

echo "Deploying application..."
kubectl apply -f chaos/local-deployment.yaml
kubectl apply -f chaos/local-service.yaml

echo "Waiting for pods to be ready..."
kubectl wait --for=condition=ready pod -l app=stratus-api --timeout=180s

echo "Deployment complete. Checking pod status..."
kubectl get pods -o wide
