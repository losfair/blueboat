FROM ubuntu:20.04

COPY ./blueboat.deb ./run.sh libmimalloc.so* /
RUN apt update && apt install -y ca-certificates /blueboat.deb && rm /blueboat.deb
ENV LD_PRELOAD=/libmimalloc.so
ENTRYPOINT /run.sh
