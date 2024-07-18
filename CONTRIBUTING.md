# Contributing to Stratus

## Set Up Environment
Check [Getting Started](README.md#getting-started-with-stratus) session.

## Running End-to-End Tests

Our end-to-end tests are located in the e2e directory. To run these tests, navigate to the e2e directory and run the following commands:

````
just e2e-stratus
````

## Setting Up Docker

You can use Docker to build and run your project. To do this, run the following command:

````
docker-compose up --build
````

## Test Your Changes

Before submitting your changes, make sure they pass the test suite. You can run the tests with:

````
just test
````

## Issue a Pull Request
At this point, you should switch back to your master branch and make sure it's up to date with Stratus' master branch:
````
git remote add upstream git@github.com:cloudwalk/stratus.git
git checkout master
git pull upstream master
````
Then, go to GitHub and open a new pull request. Make sure to provide a detailed description of your changes.
