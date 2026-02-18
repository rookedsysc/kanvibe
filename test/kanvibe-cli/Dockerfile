# KanVibe CLI 테스트용 Docker 이미지
# 클린 Ubuntu 환경에서 kanvibe.sh 의존성 체크 및 설치 흐름을 테스트한다.
#
# 빌드:
#   docker build -t kanvibe-cli-test -f test/kanvibe-cli/Dockerfile .
#
# 실행 (의존성 체크만):
#   docker run --rm -it kanvibe-cli-test bash kanvibe.sh
#
# 실행 (풀 통합 테스트 — Docker 소켓 마운트):
#   docker run --rm -it \
#     -v /var/run/docker.sock:/var/run/docker.sock \
#     kanvibe-cli-test bash kanvibe.sh start

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV LANG=en_US.UTF-8
ENV LC_ALL=en_US.UTF-8

# 기본 패키지 설치
RUN apt-get update && apt-get install -y \
    curl \
    wget \
    git \
    tmux \
    ca-certificates \
    gnupg \
    locales \
    sudo \
    && locale-gen en_US.UTF-8 ko_KR.UTF-8 zh_CN.UTF-8 \
    && rm -rf /var/lib/apt/lists/*

# Node.js 24 설치
RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# pnpm 설치
RUN corepack enable && corepack prepare pnpm@latest --activate

# Docker CLI 설치 (데몬 없이 CLI만 — 소켓 마운트로 호스트 Docker 사용)
RUN install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu noble stable" \
       > /etc/apt/sources.list.d/docker.list \
    && apt-get update \
    && apt-get install -y docker-ce-cli docker-compose-plugin \
    && rm -rf /var/lib/apt/lists/*

# gh (GitHub CLI) 설치
RUN (type -p wget >/dev/null || apt-get install -y wget) \
    && mkdir -p -m 755 /etc/apt/keyrings \
    && out=$(mktemp) \
    && wget -nv -O$out https://cli.github.com/packages/githubcli-archive-keyring.gpg \
    && cat $out | tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null \
    && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
       > /etc/apt/sources.list.d/github-cli.list \
    && apt-get update \
    && apt-get install -y gh \
    && rm -rf /var/lib/apt/lists/*

# 작업 디렉토리 설정
WORKDIR /app

# 프로젝트 파일 복사
COPY . .

# .env 파일 생성
RUN cp .env.example .env

CMD ["bash"]
