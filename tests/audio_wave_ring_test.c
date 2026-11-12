/* The POSIX waveOut ring near full: the oldest queued buffer is retired, never the newest refused.
   The ring, its mixer pull, waveOutWrite and waveOutReset are cut out of host/posix/win32_posix.c into
   audio_wave_ring.inc. No SDL device is opened and no game is run. */
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef unsigned int UINT;
typedef unsigned short WORD;
typedef uintptr_t DWORD_PTR;
typedef char *LPSTR;
typedef unsigned long DWORD;
typedef struct wavehdr_tag {
    LPSTR lpData;
    DWORD dwBufferLength, dwBytesRecorded;
    DWORD_PTR dwUser;
    volatile DWORD dwFlags;
    DWORD dwLoops;
    struct wavehdr_tag *lpNext;
    DWORD_PTR reserved;
} WAVEHDR;
typedef void *HWAVEOUT;
typedef UINT MMRESULT;
#define MMSYSERR_NOERROR 0
#define MMSYSERR_ERROR 1
#define WHDR_DONE 0x1
#define WHDR_PREPARED 0x2
#define WHDR_INQUEUE 0x10
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

#include "audio_wave_ring.inc"

enum { N = 140 };
static unsigned char data[N][4];
static WAVEHDR h[N];
static unsigned char out[N * 4];

int main(void) {
    WaveOut w;
    memset(&w, 0, sizeof w);
    pthread_mutex_init(&w.mu, NULL);
    for (int i = 0; i < N; i++) {
        memset(data[i], i, sizeof data[i]);
        h[i].lpData = (LPSTR)data[i], h[i].dwBufferLength = sizeof data[i], h[i].dwFlags = WHDR_PREPARED;
    }

    /* 60 fit; the 61st retires the oldest rather than refusing itself */
    for (int i = 0; i < 60; i++) CHECK(waveOutWrite(&w, &h[i], sizeof h[i]) == MMSYSERR_NOERROR);
    CHECK(w.count == 60 && w.head == 0 && w.pos == 0);
    CHECK((h[0].dwFlags & WHDR_INQUEUE) && !(h[0].dwFlags & WHDR_DONE));
    CHECK(waveOutWrite(&w, &h[60], sizeof h[60]) == MMSYSERR_NOERROR);
    CHECK(w.count == 60 && w.head == 1 && w.pos == 0);
    CHECK(!(h[0].dwFlags & WHDR_INQUEUE) && (h[0].dwFlags & WHDR_DONE));   /* (the oldest is the one retired) */
    CHECK((h[1].dwFlags & WHDR_INQUEUE) && !(h[1].dwFlags & WHDR_DONE));
    CHECK((h[60].dwFlags & WHDR_INQUEUE) && !(h[60].dwFlags & WHDR_DONE));

    /* A long stall keeps accepting writes: each retires exactly one, and the ring never grows past 60 */
    for (int i = 61; i < N; i++) {
        CHECK(waveOutWrite(&w, &h[i], sizeof h[i]) == MMSYSERR_NOERROR);
        CHECK(w.count == 60);
        CHECK(w.head == (i - 59) % 64);
        CHECK((h[i - 60].dwFlags & WHDR_DONE) && !(h[i - 60].dwFlags & WHDR_INQUEUE));
        CHECK((h[i].dwFlags & WHDR_INQUEUE) && !(h[i].dwFlags & WHDR_DONE));
    }

    /* the mixer takes the queued buffers in order, the retired ones first: h80 .. h139 */
    memset(out, 0xFF, sizeof out);
    wave_pull(&w, out, 60 * 4);
    CHECK(w.count == 0 && w.head == N % 64 && w.pos == 0);
    for (int i = 0; i < 60; i++)
        CHECK(out[i * 4] == (unsigned char)(N - 60 + i) && (h[N - 60 + i].dwFlags & WHDR_DONE));

    /* waveOutReset releases whatever is left */
    for (int i = 0; i < 10; i++) CHECK(waveOutWrite(&w, &h[i], sizeof h[i]) == MMSYSERR_NOERROR);
    CHECK(w.count == 10);
    waveOutReset((HWAVEOUT)&w);
    CHECK(w.count == 0 && w.pos == 0);
    for (int i = 0; i < 10; i++) CHECK((h[i].dwFlags & WHDR_DONE) && !(h[i].dwFlags & WHDR_INQUEUE));
    pthread_mutex_destroy(&w.mu);
    puts("PASS: a near-full waveOut ring retires its oldest buffer, keeps the newest and drains in order");
    return 0;
}
