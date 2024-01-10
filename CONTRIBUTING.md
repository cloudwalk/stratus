# 🤝 Contributing to Stratus 🚀

First off, thank you 🙏 for considering contributing to Stratus! It's people like you that will make Stratus our next breakthrough!

## 🤷 Where do I go from here?

If you've noticed a bug 🐛 or have a feature request 💡, make sure to open a GitHub issue if one does not already exist. If it's a fresh issue/feature, go ahead and open a new one.

## 🍴 Fork & create a branch

If this is something you think you can fix, then fork Stratus and create a branch with a descriptive name.

A good branch name would be (where issue #325 is the ticket you're working on):

````sh
git checkout -b feature/325_add_japanese_locale
````

# 🏃‍♀️ Get the test suite running
Make sure you're able to run the test suite. Details on how to do that should be in the README.md.

# 🛠️ Make your change
Once you're all set up, make your change. Here, have fun and enjoy!

# 👀 View your changes
Make sure your changes pass the test suite and look good!

# 📮 Issue a Pull Request
At this point, you should switch back to your master branch and make sure it's up to date with Stratus' master branch:
````
git remote add upstream git@github.com:original/stratus.git
git checkout master
git pull upstream master
````

Then update your feature branch from your local copy of master, and push it!

````
git checkout feature/325_add_japanese_locale
git rebase master
git push --set-upstream origin feature/325_add_japanese_locale
````

Finally, go to GitHub and make a Pull Request :D

# 🔄 Keeping your Pull Request updated
If a maintainer asks you to "rebase" your PR, they're saying that a lot of code has changed, and that you need to update your branch so it's easier to merge.

To learn more about rebasing in Git, there are a lot of good resources but here's the suggested workflow:
````
git checkout feature/325_add_japanese_locale
git pull --rebase upstream master
git push --force-with-lease feature/325_add_japanese_locale
````

# 🤝 Merging a PR (maintainers only)
A PR can only be merged into master by a maintainer if:

- It is passing CI.
- It has been approved by at least two maintainers. If it was a maintainer who opened the PR, only one extra approval is needed.
- It has no requested changes.
- It is up to date with current master.

Any maintainer is allowed to merge a PR if all of these conditions are met.

# 🎉 Join Us!
We're on a mission to make the best interplanetary EVM executor and JSON-RPC server out there. Want to join us? We're a unicorn company, billions in valuation, hundreds of millions in revenue (400m ARR), profitable (10% net income margins) and a hardcore engineering team moving fast, with no traditional BS you face in most of the startups (+500 people spread around +15 countries). We'd love to have you on board! If you feel the call, please open an issue in our project!