# OpenDeck Ulanzi D200(H) Driver

An unofficial plugin for the Ulanzi D200 family

If you have any problem related to the connection to the device or if the device is not rendering correctly you can open an issue on the GitLab project.

If you are in the Github project, this is only a mirror.

## Supported devices

- Ulanzi D200
- Ulanzi D200H

## Platform support

There are moore supported platforms in the roadmap and you can help if you want.

Currently only the following platforms are supported:

- Linux: Working and being currently development for

## Installation

Download an archive from releases

In OpenDeck: Plugins -> Install from file

## Building

You'll need rust and cargo to build this project:

#### Debug Version

```sh
$ sh pack.sh
```

#### Release Version

```sh
$ sh pack.sh release 
```

## Road Map

The road map is really short because the plug-in is already working withou any problems and all the main features are already done

### v0.6.3
 - Support for GPU load in status window #8
 - Better organization in code
 - Change in plugin namming, internal and external #11
 - Stability updates
 - Launch on OpenDeck Store #10

### v1.0.0
 - Being tested by more people
 - Better icon for the Screen Switch action
 - Better naming for actions and categories
 - Stability updates

### Sometime in the future
 - Support for macOS
 - Support for Windows