FROM ubuntu:20.04

COPY ./blueboat.deb ./run.sh /
RUN apt update && apt install -y ca-certificates /blueboat.deb
RUN rm /blueboat.deb
ENTRYPOINT /run.sh
