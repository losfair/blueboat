FROM losfair/rusty-workers-baseenv

COPY rusty-workers-fetchd /usr/bin/

EXPOSE 3000/tcp
ENTRYPOINT /usr/bin/rusty-workers-fetchd --rpc-listen 0.0.0.0:3000
