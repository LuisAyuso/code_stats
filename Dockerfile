FROM ubuntu:18.04

RUN apt-get update && apt-get install -y \
        libclang-dev \
        cargo  \
    && rm -rf /var/lib/apt/lists/*
