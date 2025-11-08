DOCKER_NAME ?= rcore-docker
.PHONY: docker build_docker
	
docker:
	docker run --network host --rm -it -v ${PWD}:/mnt -w /mnt ${DOCKER_NAME} bash

build_docker: 
	docker build -t ${DOCKER_NAME} .

fmt:
	cd easy-fs; cargo fmt; cd ../easy-fs-fuse; cargo fmt; cd ../os ; cargo fmt; cd ../user; cargo fmt; cd ..

clean:
	cd easy-fs; cargo clean; cd ../easy-fs-fuse; cargo clean; cd ../os ; cargo clean; cd ../user; cargo clean; cd ..

clippy:
	cd easy-fs; cargo clippy; cd ../easy-fs-fuse; cargo clippy; cd ../os ; cargo clippy; cd ../user; cargo clippy; cd ..
