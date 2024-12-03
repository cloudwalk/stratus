import os
import signal
import shutil
import json
import time
import subprocess
import requests
from pathlib import Path

# Configuration
SRC_CONFIG_PATH = "../../src/eth/storage/permanent/rocks/rocks_config.rs"
CONFIGS_FOLDER = "./configs"  # folder containing different config files
BENCHMARK_SERVICE_IP = "http://10.52.184.7:3232"
RESULTS_DIR = "benchmark_results"

def replace_config(config_file):
    """Replace the config file in src with the provided one"""
    shutil.copy(config_file, SRC_CONFIG_PATH)
    print(f"Replaced config with {config_file}")

def run_migration():
    """Run the database migration"""
    process = subprocess.run(
        ["cargo", "run", "--release", "--bin", "rocks_migration", "--", "../../old_data/rocksdb", "../../data/rocksdb"],
        env=dict(os.environ, RUST_LOG="info"),
        check=True,
        stdout=open("migration.log", "w")
    )
    print("Migration completed")

def run_stratus():
    """Run the stratus program and return the process"""
    process = subprocess.Popen(
        ["cargo", "run", "--release", "--bin", "stratus", "--", "--leader"],
        preexec_fn=os.setsid,  # Creates a new process group,
        cwd="../../",
        stdout=open("stratus.log", "w"),
    )
    print("Started stratus")
    # Give it some time to start up
    print("Waiting for stratus to start...")
    port_open = False
    while not port_open:
        try:
            result = subprocess.run(["lsof", "-i", ":3000"], capture_output=True, text=True)
            if result.stdout:
                port_open = True
            else:
                time.sleep(1)
        except subprocess.CalledProcessError:
            time.sleep(1)
    print("Port 3000 is now in use, stratus has started")
    return process

def start_benchmark():
    """Start the benchmark on the external service"""
    benchmark_data = {
        "target_account_strategy": "Random",
        "cashier_contract_address": "0x6ac607aBA84f672C092838a5c32c22907765F666",
        "private_keys_file": "keys_new.txt",
        "http_address": "http://10.52.184.4:3000",
        "amp_step": 100,
        "ramp_interval": 1,
        "run_duration": 600
    }

    response = requests.post(
        f"{BENCHMARK_SERVICE_IP}/start",
        json=benchmark_data,
        headers={"Content-Type": "application/json"}
    )
    response.raise_for_status()
    print("Benchmark started")

def wait_for_benchmark():
    """Poll the benchmark service until completion and return results"""
    while True:
        response = requests.get(f"{BENCHMARK_SERVICE_IP}/status")
        response.raise_for_status()
        status = response.json()

        if not status["is_running"]:
            print("Benchmark completed")
            return status["latest_results"]

        time.sleep(10)  # Poll every 10 seconds

def save_results(config_file: Path, results):
    """Save the results and config to a new directory"""
    # Create unique directory name based on timestamp
    timestamp = time.strftime("%Y%m%d_%H%M%S")
    result_dir = Path(RESULTS_DIR) / f"run_{timestamp}"
    result_dir.mkdir(parents=True, exist_ok=True)

    # Copy config file
    shutil.copy(config_file, result_dir / config_file)

    # Save results
    with open(result_dir / "results.json", "w") as f:
        json.dump(results, f, indent=2)

    print(f"Results saved in {result_dir}")

def main():
    # Create results directory if it doesn't exist
    Path(RESULTS_DIR).mkdir(exist_ok=True)

    # Get all config files
    config_files = list(Path(CONFIGS_FOLDER).glob("*.rs"))

    for config_file in sorted(config_files):
    # Delete files in the previous database
        db_path = Path("../../data")
        if db_path.exists():
            for item in db_path.iterdir():
                if item.is_file():
                    item.unlink()
                elif item.is_dir():
                    shutil.rmtree(item)
            print("Cleared previous database contents")
        db_path.mkdir(parents=True, exist_ok=True)

        print(f"\nTesting config: {config_file}")
        try:
            # 1. Replace config
            replace_config(config_file)

            # 2. Run migration
            run_migration()

            # 3. Start the program
            stratus_process = run_stratus()

            try:
                # 4. Start benchmark
                start_benchmark()

                # 5. Wait for benchmark completion
                results = wait_for_benchmark()

                # 6. Save results and stop program
                save_results(config_file, results)

            finally:
                # Ensure we always try to stop the program
                if stratus_process:
                    # Send SIGTERM to the process group
                    os.killpg(os.getpgid(stratus_process.pid), signal.SIGTERM)
                    stratus_process.wait()
                    print("Stratus stopped")

        except Exception as e:
            print(f"Error processing {config_file}: {e}")
            continue

if __name__ == "__main__":
    main()
