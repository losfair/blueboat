FROM losfair/rusty-workers-baseenv

RUN apt install -y nginx curl

COPY rusty-workers-fetchd /usr/bin/
COPY rusty-workers-proxy /usr/bin/
COPY rusty-workers-runtime /usr/bin/
COPY docker-test/boot.sh /
COPY docker-test/config.toml /var/www/html/
COPY docker-test/hello.js /var/www/html/

EXPOSE 8080/tcp
ENTRYPOINT /boot.sh
