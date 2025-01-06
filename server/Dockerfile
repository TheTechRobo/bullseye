# Set the HOST env to 0.0.0.0 in most situations.
# This Dockerfile must have its context in the parent dir,
# ie, in the root of the repo, use -f to specify this Dockerfile.

FROM rust:1.82-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig protobuf-dev

WORKDIR /usr/src/myapp
COPY . bullseye
WORKDIR /usr/src/myapp/bullseye/server
RUN cargo install --path .

FROM alpine

RUN mkdir /data
WORKDIR /data

COPY --from=builder /usr/local/cargo/bin/bullseye-server /bullseye-server

ENTRYPOINT ["/bullseye-server"]
