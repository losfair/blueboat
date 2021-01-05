FROM ubuntu:20.04

COPY rusty-workers-runtime /usr/bin/
RUN apt update
RUN apt install -y libssl1.1

EXPOSE 3000/tcp
ENTRYPOINT /usr/bin/rusty-workers-runtime --rpc-listen 0.0.0.0:3000
