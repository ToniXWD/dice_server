#!/bin/bash

docker run --rm -p 4317:4317 -p 8888:8888 -p 8889:8889 -p 14268:14268 -p 14250:14250 -p 55680:55680 -p 13133:13133 -p 16686:16686 otel/opentelemetry-collector-contrib:latest
