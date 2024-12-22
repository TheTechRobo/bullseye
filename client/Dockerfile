# Writes the binary to /bullseye-client

FROM rust:1.82 AS builder

RUN mkdir /output
WORKDIR /usr/src/myapp
RUN git clone https://github.com/TheTechRobo/bullseye
WORKDIR /usr/src/myapp/bullseye/client
RUN cargo install --path .

FROM scratch

COPY --from=builder /usr/local/cargo/bin/bullseye-client /bullseye-client

