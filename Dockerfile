FROM docker.io/rust:1.79

WORKDIR /usr/src/matrix-wip-bot
COPY . .

RUN cargo install --path .

CMD ["matrix-wip-bot"]
