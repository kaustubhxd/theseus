/*
 * sbk2sf2: convert a SoundFont 1 bank (.SBK, AWE32) to SoundFont 2 (.SF2).
 * Written for the Road Rash evaluation on top of awesfx (Takashi Iwai, GPL):
 * awesfx's loader reads the SBK, its sbk_to_sf2() converts each generator
 * value to SF2 units, and its writer produces the file once the version is
 * set to 2. Additions here:
 *  - the SBK-only samplePitch generator (slot 55, reserved in SF2) is moved
 *    into the referenced sample header (originalPitch / pitchCorrection);
 *  - SF2 terminator records are added where missing (EOS sample);
 *  - the INFO 'ifil' version copied from the original is patched to 2.01.
 * ROM samples (AWE32 built-in) can't be included; they're reported.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "sffile.h"

extern int sbk_to_sf2(int oper, int amount);
#define GEN_INSTRUMENT 41
#define GEN_SAMPLEID 53
#define GEN_SAMPLEPITCH 55

static void convert_layers(SFHeader *h, SFInfo *sf, int is_inst)
{
	int j, k, n;
	for (j = 0; j < h->nlayers; j++) {
		SFGenLayer *l = &h->layer[j];
		int sample = -1, pitch = -1;
		for (k = 0, n = 0; k < l->nlists; k++) {
			SFGenRec g = l->list[k];
			if (g.oper == GEN_SAMPLEID) sample = g.amount;
			if (g.oper == GEN_SAMPLEPITCH) { pitch = g.amount; continue; }
			if (g.oper != GEN_INSTRUMENT && g.oper != GEN_SAMPLEID)
				g.amount = sbk_to_sf2(g.oper, g.amount);
			l->list[n++] = g;
		}
		l->nlists = n;
		if (is_inst && sample >= 0 && sample < sf->nsamples && pitch >= 0) {
			sf->sample[sample].originalPitch = pitch / 100;
			sf->sample[sample].pitchCorrection = pitch % 100;
		}
	}
}

int main(int argc, char **argv)
{
	SFInfo sf;
	FILE *fin, *fout;
	int i, rom = 0;
	if (argc != 3) { fprintf(stderr, "usage: sbk2sf2 in.sbk out.sf2\n"); return 1; }
	if (!(fin = fopen(argv[1], "rb"))) { perror(argv[1]); return 1; }
	memset(&sf, 0, sizeof(sf));
	if (awe_load_soundfont(&sf, fin, 1) < 0) { fprintf(stderr, "can't load %s\n", argv[1]); return 1; }
	if (sf.version != 1) { fprintf(stderr, "%s is not SoundFont 1 (version %d)\n", argv[1], sf.version); return 1; }
	for (i = 0; i < sf.ninsts; i++) convert_layers(&sf.inst[i].hdr, &sf, 1);
	for (i = 0; i < sf.npresets; i++) convert_layers(&sf.preset[i].hdr, &sf, 0);
	for (i = 0; i < sf.nsamples; i++) {
		if (sf.sample[i].sampletype & 0x8000) rom++;
		if (sf.sample[i].name[0] == 0) snprintf(sf.sample[i].name, 20, "sample%d", i);
	}
	/* SF2 needs a terminal "EOS" sample header */
	if (sf.nsamples == 0 || strncmp(sf.sample[sf.nsamples - 1].name, "EOS", 3) != 0) {
		sf.sample = realloc(sf.sample, sizeof(SFSampleInfo) * (sf.nsamples + 1));
		memset(&sf.sample[sf.nsamples], 0, sizeof(SFSampleInfo));
		strcpy(sf.sample[sf.nsamples].name, "EOS");
		sf.nsamples++;
	}
	sf.version = 2; sf.minorversion = 1;
	if (!(fout = fopen(argv[2], "wb+"))) { perror(argv[2]); return 1; }
	awe_save_soundfont(&sf, fin, fout);
	/* patch ifil (copied verbatim from the SF1 INFO list) to 2.01 */
	{
		long len; char *buf;
		fflush(fout); fseek(fout, 0, SEEK_END); len = ftell(fout); fseek(fout, 0, SEEK_SET);
		buf = malloc(len); fread(buf, 1, len, fout);
		for (i = 0; i + 12 <= len; i++)
			if (memcmp(buf + i, "ifil", 4) == 0) {
				buf[i + 8] = 2; buf[i + 9] = 0; buf[i + 10] = 1; buf[i + 11] = 0; break;
			}
		fseek(fout, 0, SEEK_SET); fwrite(buf, 1, len, fout); free(buf);
	}
	fclose(fout); fclose(fin);
	printf("%s -> %s: %d presets, %d instruments, %d samples%s\n", argv[1], argv[2],
	       sf.npresets, sf.ninsts, sf.nsamples - 1, rom ? " (has AWE32 ROM samples: not included)" : "");
	return 0;
}
