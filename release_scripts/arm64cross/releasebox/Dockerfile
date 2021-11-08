FROM ubuntu:20.04

COPY ./run.sh /
COPY ./blueboat.deb /
RUN apt update && apt install -y ca-certificates /blueboat.deb && rm /blueboat.deb
ENTRYPOINT /run.sh
