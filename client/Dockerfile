# Writes the binary to /bullseye-client
# This Dockerfile must have its context in the parent dir,
# ie, in the root of the repo, use -f to specify this Dockerfile.

FROM rust:1.82-alpine AS builder

RUN apk add --no-cache git musl-dev pkgconfig

WORKDIR /usr/src/myapp
COPY . bullseye
WORKDIR /usr/src/myapp/bullseye/client
RUN cargo install --path .

FROM scratch

COPY --from=builder /usr/local/cargo/bin/bullseye-client /bullseye-client

