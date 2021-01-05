FROM ubuntu:20.04

COPY rusty-workers-fetchd /usr/bin/
RUN apt update
RUN apt install -y libssl1.1

EXPOSE 3000/tcp
ENTRYPOINT /usr/bin/rusty-workers-fetchd --rpc-listen 0.0.0.0:3000
