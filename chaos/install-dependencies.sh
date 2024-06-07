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

echo "Building Docker image..."
docker build -t stratus-api --file ./docker/Dockerfile.run_stratus .

echo "Loading Docker image into Kind..."
kind load docker-image stratus-api --name local-testing

echo "Deploying application..."
kubectl apply -f chaos/local-deployment.yaml
kubectl apply -f chaos/local-service.yaml

echo "Waiting for pods to be ready..."
kubectl wait --for=condition=ready pod -l app=stratus-api --timeout=180s

echo "Deployment complete. Checking pod status..."
kubectl get pods -o wide
