FROM rust:slim

LABEL maintainer="Collin Pendleton <collinp@collinpendleton.com>"

ARG DEBIAN_FRONTEND=noninteractive

# Create location where pinepods code is stored
# RUN mkdir /pinepods
# Make sure the package repository is up to date. Also install needed packages via apt
RUN apt update && \
    apt -qy upgrade && \
    apt install -qy git software-properties-common curl cron supervisor gcc libffi-dev zlib1g-dev libjpeg-dev libavformat-dev libavformat-dev libavutil-dev ffmpeg clang libasound2-dev libclang-dev && \
    rm -rf /var/lib/apt/lists/*


# Put pinepods Files in place
# Create structure for pinepods
RUN git clone https://github.com/madeofpendletonwool/pinepods-firewood.git /pinepods-firewood && \
    chmod -R 755 /pinepods-firewood

# Install needed rust packages via cargo
RUN cd /pinepods-firewood && cargo init && cargo build

# Add a cache-busting build argument
ARG CACHEBUST=1

# Begin pinepods Setup
ADD startup.sh /
RUN ls -al /
ENTRYPOINT ["/startup.sh"]
