# Contributing to cpkg Registry

Thank you for contributing to the cpkg ecosystem.

This registry allows developers to publish apps via pull request using a JSON manifest

---

## Adding a new app

To register your app, create a JSON file in the `/apps` directory using this naming format:
```
GithubRepoName.GithubUsername.json
```
an example for the user callen with a repo called explorer would be explorer.callen.json


---

## Required format

Each file must contain valid JSON with the following structure:

```json
{
  "name": "AppName",
  "repo": "https://github.com/GithubUsername/Repository",
  "description": "Example app description",
  "download": "https://github.com/GithubUsername/Repository/releases/latest/download/installer.exe"
}
```
stay consistent with the name of the installer

another example is the brave browser nightly installer
```json
{
  "name": "Brave Browser",
  "repo": "https://github.com/brave/brave-browser",
  "description": "Brave Browser is a lightning fast, safe and private web browser that prevents you from being tracked and blocks ads by default.",
  "download": "https://github.com/brave/brave-browser/releases/latest/download/BraveBrowserStandaloneSilentNightlySetup.exe"
}
```