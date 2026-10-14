FROM fedora:42
ENV container=podman
COPY init-settings.sh /usr/local/bin/init-replay-control-test-settings
RUN dnf install -y rpm-build systemd curl python3 cpio && dnf clean all
RUN sh /usr/local/bin/init-replay-control-test-settings /etc/replay-control
CMD ["sleep", "infinity"]
