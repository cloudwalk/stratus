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

echo "Checking if Docker image is already built..."
if ! docker images | grep -q local/run_with_importer; then
    echo "Building Docker image..."
    docker build -t local/run_with_importer -f ./docker/Dockerfile.run_with_importer .
else
    echo "Docker image already built."
fi

echo "Loading Docker image into Kind..."
kind load docker-image local/run_with_importer --name local-testing

echo "Deploying application..."
kubectl apply -f chaos/local-deployment.yaml
kubectl apply -f chaos/local-service.yaml

echo "Waiting for pods to be ready..."
kubectl wait --for=condition=ready pod -l app=stratus-api --timeout=180s

echo "Deployment complete. Checking pod status..."
kubectl get pods -o wide