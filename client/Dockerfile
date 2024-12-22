# Writes the binary to /output/bullseye-client

FROM rust:1.82 AS builder

RUN mkdir /output
WORKDIR /usr/src/myapp
RUN git clone https://github.com/TheTechRobo/bullseye
WORKDIR /usr/src/myapp/bullseye/client
RUN cargo install --path .
RUN cp /usr/local/cargo/bin/bullseye-client /output/bullseye-client

ENTRYPOINT ["/output/bullseye-client"]

