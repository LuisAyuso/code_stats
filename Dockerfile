FROM frolvlad/alpine-rust

RUN apk add --no-cache clang-dev

#ENV LIBCLANG_PATH /usr/lib/llvm-6.0/lib/ 

COPY . /code
RUN cd /code && cargo build --release

ENV PATH "/code/target/release/:${PATH}"