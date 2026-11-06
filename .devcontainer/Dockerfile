FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

ENV BUN_INSTALL=/home/vscode/.bun
ENV PATH=${BUN_INSTALL}/bin:${PATH}

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        file \
        openssl \
        python3 \
        libayatana-appindicator3-dev \
        libgtk-3-dev \
        libssl-dev \
        librsvg2-dev \
        libwebkit2gtk-4.1-dev \
        libxdo-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

USER vscode

RUN curl -fsSL https://bun.sh/install | bash