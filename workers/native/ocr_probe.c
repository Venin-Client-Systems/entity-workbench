/* Synthetic confinement fixture only. Never included in the application runtime. */
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

static int read_file(const char *name) {
    int fd = open(name, O_RDONLY); if (fd < 0) return 0;
    char byte; int ok = read(fd, &byte, 1) == 1; close(fd); return ok;
}
static int write_file(const char *name) {
    int fd = open(name, O_WRONLY | O_APPEND); if (fd < 0) return 0;
    int ok = write(fd, "X", 1) == 1; close(fd); return ok;
}
int main(int argc, char **argv) {
    if (argc < 3) return 80;
    FILE *input = fopen(argv[1], "r"); if (!input) return 81;
    char mode[32], outside[4096], index[4096]; int port, descriptor;
    if (!fgets(mode, sizeof(mode), input)) return 82;
    char output[4096]; if (snprintf(output, sizeof(output), "%s.txt", argv[2]) >= (int)sizeof(output)) return 83;
    FILE *result = fopen(output, "w"); if (!result) return 84;
    if (!strcmp(mode, "sleep\n")) {
        fprintf(result, "%d\n", getpid()); fflush(result);
        for (;;) sleep(1);
    }
    if (!strcmp(mode, "overflow\n")) {
        for (int i = 0; i < 600000; i++) if (fputc('x', result) == EOF) return 85;
        return fclose(result) == 0 ? 0 : 86;
    }
    if (!fgets(outside, sizeof(outside), input) || !fgets(index, sizeof(index), input) || fscanf(input, "%d %d", &port, &descriptor) != 2) return 87;
    outside[strcspn(outside, "\n")] = 0; index[strcspn(index, "\n")] = 0; fclose(input);
    int fd = socket(AF_INET, SOCK_STREAM, 0), network = 0;
    if (fd >= 0) {
        struct sockaddr_in address = {0}; address.sin_family = AF_INET; address.sin_port = htons((unsigned short)port); address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        network = connect(fd, (struct sockaddr *)&address, sizeof(address)) == 0; close(fd);
    }
    pid_t child = fork(); if (child == 0) _exit(0);
    char value; int inherited = read(descriptor, &value, 1) == 1;
    fprintf(result, "outside_read=%d outside_write=%d index_read=%d index_write=%d input_write=%d network=%d fork=%d inherited=%d environment=%d\n", read_file(outside), write_file(outside), read_file(index), write_file(index), write_file(argv[1]), network, child >= 0, inherited, getenv("WORKBENCH_TEST_SECRET") != NULL);
    return fclose(result) == 0 ? 0 : 88;
}
