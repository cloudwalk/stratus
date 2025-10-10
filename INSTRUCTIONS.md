# Connect to benchmark nodes

```shell
gcloud compute ssh "stratus-use4-bench-0" --zone "us-east4-b" --project "infinitepay-staging" --tunnel-through-iap
gcloud compute ssh "stratus-use4-bench-1" --zone "us-east4-b" --project "infinitepay-staging" --tunnel-through-iap
```

```shell
curl -X POST 10.52.184.7:3232/benchmark/pr/####
```

# Disable GCP Ops Agent

```shell
sudo systemctl stop google-cloud-ops-agent
sudo systemctl disable google-cloud-ops-agent
```

# Enable GCP Ops Agent

```shell
sudo systemctl enable google-cloud-ops-agent
sudo systemctl start google-cloud-ops-agent
```

For testing use:

```shell
sudo systemctl status google-cloud-ops-agent
```