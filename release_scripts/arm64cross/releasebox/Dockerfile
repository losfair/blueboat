FROM ubuntu:20.04

COPY ./run.sh /
RUN apt update && apt install -y ca-certificates /blueboat.deb
ENTRYPOINT /run.sh
