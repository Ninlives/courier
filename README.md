# courier

A follow-only ActivityPub relay that carries supplementary data from other instances to your server.

## Credits

This project starts as a fork of [buzzrelay](https://github.com/astro/buzzrelay), a large part of the content are directly inherited from that project.
Many thanks to @Astro for sharing his amazing work.

## Introduction

`courier` will try its best to carry the following data to your server:

1. **Completion**: The replies to posts appeared on the global timeline.
   Since ActivityPub does not define a collection for post replies (I hope there will be one in the future),
   most implementations will not proactively fetch replies for posts that is originated from other instances.
   Therefore, users may not see all replies to a post, unless all participants are followed by someone in the current server, or jump to the original server to get a complete view.
   This is the main reason why I develop `courier`.
   `courier` will try to fetch all replies from remote and send them to the current server, so no need to jump across different instances.
2. **Trends**: The trending posts on other instances.
   This feature is designed for small instances that do not have a large amount of users, but still want to see what's trending in the Fediverse.

## Setup

**Disclaimer**:
`courier` is still work in progress, and I have only tested it on my Misskey instance.
If you are experiencing any problems and would like to help with development, please submit an issue!

**For instance moderators**:
All data sent by `courier` is wrap in activities with type `["Announce", "Relay"]`,
which will be translated into retoots or renotes in most implementations.
I did not find a more elegant way to do this, so if you do not want your server's timeline be flooded with retoots,
try globally mute actors from `courier`, or patch the server to recognize the `Relay` type.
I use [this patch](./misskey-relay.patch) for my Misskey instance.

### Build

NixOS/Flakes users are in luck: not only does this build, it also
comes with a NixOS module!

Anyone else installs a Rust toolchain to build with:

```bash
cargo build --release
```

### Generate signing keypair

ActivityPub messages are signed using RSA keys. Generate a keypair
first:

```bash
openssl genrsa -out private-key.pem 4096
openssl rsa -in private-key.pem -pubout -out public-key.pem
```

Let your `config.yaml` point there.

### Database

Create a PostgreSQL database and user, set them in your `config.yaml`.

The program will create its schema on start.
