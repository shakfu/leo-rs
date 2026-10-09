# The paper's chunks, run as a script; `make render` needs rmarkdown.

RSCRIPT ?= Rscript

.PHONY: test render clean

test:
	$(RSCRIPT) growth.R

render:
	$(RSCRIPT) -e 'rmarkdown::render("growth.Rmd")'

clean:
	rm -rf growth.html growth_files
