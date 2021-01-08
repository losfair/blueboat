FROM losfair/rusty-workers-baseenv

RUN apt install -y nginx curl

COPY rusty-workers-fetchd /usr/bin/
COPY rusty-workers-proxy /usr/bin/
COPY rusty-workers-runtime /usr/bin/
COPY docker-test/boot.sh /
COPY docker-test/config.toml /var/www/html/

RUN mkdir /tmp/pack
COPY docker-test/hello.js /tmp/pack/index.js
RUN cd /tmp/pack && tar c . > /var/www/html/hello.tar

EXPOSE 8080/tcp
ENTRYPOINT /boot.sh
