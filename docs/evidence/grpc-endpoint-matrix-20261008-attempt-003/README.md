# Endpoint matrix building

Source `50ac70e5c9818fe870719b37acede99f12710fe9`. This separate attempt supplies the installed protoc 35.1 on PATH. The previous failed build is retained in [attempt 002](../grpc-endpoint-matrix-20261008-attempt-002/README.md).

The unchanged controller builds and freezes the release binary, then runs the requested 2,560 functional cells and separate native/Callgrind N/2N captures. It retains every failure and is configured to publish the outcome directly to main. The controller and release build were running when launch.json was captured. No completed measurement is claimed; qualification remains false.
