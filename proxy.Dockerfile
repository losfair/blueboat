FROM losfair/rusty-workers-baseenv

COPY rusty-workers-proxy /usr/bin/

EXPOSE 8080/tcp
ENTRYPOINT /usr/bin/rusty-workers-proxy --http-listen 0.0.0.0:8080
