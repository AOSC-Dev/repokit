# AOSC OS Tarball / Image Metadata Generator

This is the AOSC OS image metadata generator. It generates the image metadata for the tarballs in JSON format.
The generated format is documented at https://app.swaggerhub.com/apis-docs/liushuyu/DeployKit/1.0#.

This project is part of the AOSC infrastructures.

## Building

Just run `cargo build --release` and wait.

## Usage

First you need to create a configuration file. Refer to `example.toml` in this repository for more information.

Then run `./repo-manifest -c <path/to/config.toml>` to start.
