FROM losfair/rusty-workers-baseenv

COPY rusty-workers-runtime /usr/bin/

EXPOSE 3000/tcp
ENTRYPOINT /usr/bin/rusty-workers-runtime --rpc-listen 0.0.0.0:3000
