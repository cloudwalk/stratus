# ☁️ Stratus: The Interplanetary EVM Executor & JSON-RPC Server ☁️

Welcome to **Stratus**! Dive into the world of an interplanetary EVM executor and JSON-RPC server. Crafted with the robustness of Rust 🦀, Stratus stands out with its custom, horizontally scalable storage. Growth is our game! 📈

## 🚀 Our Journey 🚀

Back in 2016, CloudWalk, Inc. embarked on a mission to harness Ethereum for building our payment acquirer. After exploring various networks and private ledgers, we hit a roadblock with scaling issues. Enter **Stratus**: our very own EVM ledger solution, now open-sourced for all. Stratus is our stepping stone to an interplanetary payment network, ready to process trillions of transactions, both on Earth and beyond.

## 🗃️ Our Storage Solutions 🗃️

Stratus offers diverse storage implementations, catering to various needs:

- **In Memory**: Embrace the speed with our ephemeral storage solution.
- **PostgreSQL**: Rely on the robustness of this time-tested system.

## 🌌 What's on the Horizon for Stratus? 🌌

At CloudWalk, we're constantly looking forward. Here's what Stratus is gearing up for:

- **Redis Integration**: Prepare for ultra-fast, versatile storage.
- **CockroachDB**: The resilient, cloud-friendly DB is joining our suite.
- **L2 Proof Mechanisms to Ethereum Mainnet**: Our step towards decentralized efficiency with amazing latency.

Exciting developments await!

## 🤲 How to Contribute 🤲

We welcome contributions from everyone, experts and beginners alike! Join us in refining Stratus. Check our `CONTRIBUTING.md` to get started.

## 🚀 Getting Started with Stratus 🚀

Running Stratus is straightforward. Follow these steps to get it up and running on your system.

> Before you begin, ensure you have Rust and Node.js installed. For Rust, use this command:
>
> ```bash
> curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
> source $HOME/.cargo/env
> ```
>
> Make sure your Node.js version is 16:
>
> ```bash
> nvm install 16
> nvm use 16
> ```


## 📌 How can I run Stratus?

1. **Set up Just**
   ```bash
   mkdir -p ~/bin
   curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to ~/bin
   export PATH="$PATH:$HOME/bin"

1. **Clone the Repository**
   ```bash
   > git clone https://github.com/cloudwalk/stratus.git
   ```

2. **Navigate to the Repository**
   ```bash
   > cd stratus
   ```

3. **Run Stratus with Just**
   ```bash
   > just e2e-stratus
   ```

## Join Our Mission

Join us in shaping the finest interplanetary EVM executor and JSON-RPC server. As a unicorn company valued in billions, with a revenue of 400m ARR and 10% net income margins, CloudWalk fosters a dynamic engineering team across +15 countries. We're over 500 strong, avoiding traditional startup pitfalls. Feel the call? Open an issue in our project and embark on this journey with us!

## 📜 License 📜

Stratus is proudly open-source under the MIT license. This gives you the freedom to use, modify, and distribute the software as per the license terms. For more details, visit our `LICENSE` file in the repository.

**Thanks for exploring Stratus! Stay tuned for more!** ☁️
