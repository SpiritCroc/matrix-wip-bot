FROM docker.io/rust:1.82.0-bookworm

# ImageMagick - from https://github.com/nlfiedler/magick-rust/blob/master/docker/Dockerfile
# NOTE: once Debian packages imagemagick 7.1.1-26 or later, can switch to that
RUN apt-get update \
 && apt-get -y install curl build-essential clang pkg-config libjpeg-turbo-progs libpng-dev \
 && rm -rfv /var/lib/apt/lists/*

ENV MAGICK_VERSION 7.1

RUN curl https://imagemagick.org/archive/ImageMagick.tar.gz | tar xz \
 && cd ImageMagick-${MAGICK_VERSION}* \
 && ./configure --with-magick-plus-plus=no --with-perl=no \
 && make \
 && make install \
 && cd .. \
 && rm -r ImageMagick-${MAGICK_VERSION}*

# WIP-Bot

WORKDIR /usr/src/matrix-wip-bot
COPY . .

RUN cargo install --path .

CMD ["matrix-wip-bot"]
