docker buildx build --platform linux/amd64 -f Dockerfile.linux_x86_64 -t brows/x86_64-unknown-linux-gnu:local --load .
docker buildx build --platform linux/amd64 -f Dockerfile.linux_aarch64 -t brows/aarch64-unknown-linux-gnu:local --load .
docker buildx build --platform linux/amd64 -f Dockerfile.linux_armv7 -t brows/armv7-unknown-linux-gnueabihf:local --load .
