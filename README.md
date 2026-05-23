# OpenDeck Ulanzi D200(H) Driver

An unofficial plugin for the Ulanzi D200 family

If you have any problem related to the connection to the device or if the device is not rendering correctly you can open an issue on the [GitLab](https://gitlab.com/glmagalhaes.mail/rs-ulanzi-d-200-linux).

If you are in the Github project, this is only a mirror.

## Supported devices

- Ulanzi D200
- Ulanzi D200H

## Platform support

There are more supported platforms in the roadmap and you can help if you want.

Currently only the following platforms are supported:

- Linux: Working and being currently development for

## Installation

Download an archive from releases.

In OpenDeck: Plugins -> Install from file

Soon it will be available on OpenDeck Store.


## Actions

This plugin has an action that is for a native funcionality from both D200 and D200H. The wide button has 3 functions, blank, clock and PC stat. This doesn't affect the functionality as a button, it just show one of those 3 informations.

 - Screen Switch:  This will cycle between the 3 modes on click.

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
- [x] Support for GPU load in status window #8
- [ ] Better organization in code
- [x] Change in plugin namming, internal and external #11
- [ ] Stability updates
- [ ] Launch on OpenDeck Store #10

### v1.0.0
- [ ] Being tested by more people
- [ ] Better icon for the Screen Switch action
- [x] Better naming for actions and categories
- [ ] Stability updates

### Sometime in the future
- [ ] Support for macOS
- [ ] Support for Windows