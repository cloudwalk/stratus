# 🤝 Contributing to Stratus 🚀

First off, thank you 🙏 for considering contributing to Stratus! It's people like you that make Stratus our next breakthrough!

## 🤷 Where do I go from here?

If you've noticed a bug 🐛 or have a feature request 💡, make sure to open a GitHub issue if one does not already exist. If it's a fresh issue/feature, go ahead and open a new one.

## 🍴 Fork & create a branch

If this is something you think you can fix, then fork Stratus and create a branch with a descriptive name.

A good branch name would be (where issue #325 is the ticket you're working on):

```sh
git checkout -b feature/325_add_japanese_locale
```

# 🏃‍♀️ Set Up Your Environment

Before you start coding, make sure you have the Rust environment set up and the necessary dependencies installed. You can do this by running:

````sh
just build
````

Make sure your code adheres to our project's style guidelines by running rustfmt and clippy:
````
just fmt
just clippy
````

# 🗄️ Working with the Database

Our project uses sqlx for database operations. Before you can run the project or the tests, you'll need to set up a local database and add the connection string to your environment variables.

Here's how to run the database migrations:
````
just migrate
````

# 🧪 Running End-to-End Tests

Our end-to-end tests are located in the e2e directory. To run these tests, navigate to the e2e directory and run the following commands:

````
cd e2e
npm install
npx hardhat test
````

# 🐳 Setting Up Docker

You can use Docker to build and run your project. To do this, run the following command:

````
docker-compose up --build
````

# 👀 Test Your Changes

Before submitting your changes, make sure they pass the test suite. You can run the tests with:

````
just test
````

# 📮 Issue a Pull Request
At this point, you should switch back to your master branch and make sure it's up to date with Stratus' master branch:
````
git remote add upstream git@github.com:cloudwalk/stratus.git
git checkout master
git pull upstream master
````
Then, go to GitHub and open a new pull request. Make sure to provide a detailed description of your changes.