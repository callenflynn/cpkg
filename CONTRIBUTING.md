# Contributing to cpkg Registry

Thank you for contributing to the cpkg ecosystem.

This registry allows developers to publish apps via pull request using a JSON manifest

---

## Adding a new app

To register your app, create a JSON file in the `/apps` directory using this naming format:

GithubRepoName.GithubUsername.json

an example for the user callen with a repo called explorer would be explorer.callen.json


---

## Required format

Each file must contain valid JSON with the following structure:

```json
{
  "name": "AppNane",
  "repo": "https://github.com/GithubUsername/Repository",
  "description": "Example app description",
  "download": "https://github.com/GithubUsername/Repository/releases/latest/download/installer.exe"
}
```
stay consistent with the name of the installer
