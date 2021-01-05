FROM ubuntu:20.04

RUN apt update
RUN apt install -y libssl1.1 ca-certificates
