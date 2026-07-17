#!/usr/bin/env Rscript
## One-time fixture export for the `mr` crate's golden tests.
##
## Run from the workspace root:
##   Rscript bio_crates/mr/tests/data/export_fixtures.R
##
## Requires the R source of TwoSampleMR (vendored at ./TwoSampleMR) and the
## `psych` package (for `mr_steiger`'s `r.test`). It regenerates:
##   - commondata_kept.csv         (harmonised 79-SNP input for method tests)
##   - raw_exposure.csv / raw_outcome.csv  (pre-harmonisation inputs)
##   - expected_harmonised.csv     (R harmonise_data output)
##   - r_density_weighted.tsv      (R stats::density for the KDE fidelity test)
##
## The expected method `b` values live as constants in tests/*_golden.rs and in
## TwoSampleMR/tests/testthat/test_mr.R (egger 0.5025, ivw 0.4459, …).

root <- "." # workspace root
for (f in c("TwoSampleMR/R/add_rsq.r", "TwoSampleMR/R/mr.R",
            "TwoSampleMR/R/mr_mode.R", "TwoSampleMR/R/steiger.R",
            "TwoSampleMR/R/harmonise.R")) {
  e <- new.env(); sys.source(f, envir = e)
  for (n in ls(e, all.names = TRUE)) assign(n, get(n, envir = e), envir = globalenv())
}
load(file.path(root, "TwoSampleMR/inst/extdata/test_commondata.RData"))
outdir <- file.path(root, "bio_crates/mr/tests/data")

x <- dat[dat$mr_keep, ]
write.csv(
  x[, c("SNP","id.exposure","id.outcome","beta.exposure","beta.outcome",
        "se.exposure","se.outcome","mr_keep","pval.exposure","pval.outcome",
        "samplesize.exposure","samplesize.outcome","eaf.exposure","eaf.outcome")],
  file.path(outdir, "commondata_kept.csv"), row.names = FALSE
)

write.csv(
  exp_dat[, c("SNP","id.exposure","beta.exposure","se.exposure",
              "effect_allele.exposure","other_allele.exposure","eaf.exposure")],
  file.path(outdir, "raw_exposure.csv"), row.names = FALSE
)
write.csv(
  out_dat[, c("SNP","id.outcome","beta.outcome","se.outcome",
              "effect_allele.outcome","other_allele.outcome","eaf.outcome")],
  file.path(outdir, "raw_outcome.csv"), row.names = FALSE
)
write.csv(
  dat[, c("SNP","beta.exposure","beta.outcome","eaf.exposure","eaf.outcome",
          "effect_allele.exposure","other_allele.exposure",
          "effect_allele.outcome","other_allele.outcome",
          "mr_keep","remove","palindromic","ambiguous")],
  file.path(outdir, "expected_harmonised.csv"), row.names = FALSE
)

## R density for the weighted-mode KDE comparison.
b_exp <- x$beta.exposure; b_out <- x$beta.outcome
se_exp <- x$se.exposure;  se_out <- x$se.outcome
BetaIV <- b_out / b_exp
se1 <- sqrt((se_out^2)/(b_exp^2) + ((b_out^2)*(se_exp^2))/(b_exp^4))
w <- se1^-2 / sum(se1^-2)
s <- 0.9 * min(sd(BetaIV), mad(BetaIV)) / length(BetaIV)^(1/5)
d <- density(BetaIV, weights = w, bw = max(1e-8, s))
writeLines(paste(d$x, d$y, sep = "\t"), file.path(outdir, "r_density_weighted.tsv"))

cat("Fixtures written to", outdir, "\n")
