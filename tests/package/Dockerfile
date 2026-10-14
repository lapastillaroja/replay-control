ARG BASE=debian:trixie
FROM ${BASE}
ENV container=podman
COPY init-settings.sh /usr/local/bin/init-replay-control-test-settings
RUN apt-get update && apt-get install -y --no-install-recommends \
    systemd systemd-sysv dbus curl ca-certificates python3 \
    && rm -rf /var/lib/apt/lists/* \
    && sh /usr/local/bin/init-replay-control-test-settings /etc/replay-control
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
