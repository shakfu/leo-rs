<?xml version="1.0" encoding="utf-8"?>
<!-- Created by Leo: https://leo-editor.github.io/leo-editor/leo_toc.html -->
<leo_file xmlns:leo="https://leo-editor.github.io/leo-editor/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<globals/>
<preferences/>
<find_panel_settings/>
<vnodes>
<v t="a.1"><vh>About this demo</vh></v>
<v t="a.2" descendentVnodeUnknownAttributes="7d710058010000003071017d710258130000007374725f6c656f2d72732d63656c6c2d696473710358510000007365747570207374612e32303236313030393033333632382e380a666974207374612e32303236313030393033333632382e31300a6d65616e73207374612e32303236313030393033333632382e31330a710473732e"><vh>@rmd growth.Rmd</vh></v>
<v t="sta.20261009033628.14"><vh>@clean growth.R</vh>
<v t="sta.20261009033628.8"><vh>&lt;&lt; setup &gt;&gt;</vh></v>
<v t="sta.20261009033628.10"><vh>&lt;&lt; fit &gt;&gt;</vh></v>
<v t="sta.20261009033628.13"><vh>&lt;&lt; means &gt;&gt;</vh></v>
</v>
</vnodes>
<tnodes>
<t tx="a.1">An R Markdown paper whose chunks are also an R script.

@rmd growth.Rmd reads the paper. knitr's chunk labels name the nodes:
{r setup} is &lt;&lt; setup &gt;&gt;, {r fit} is &lt;&lt; fit &gt;&gt;. The unlabelled chunk is
&lt;&lt; r cell 3 &gt;&gt;; rename it to label it, and the save writes {r name}.

@clean growth.R holds clones of &lt;&lt; setup &gt;&gt;, &lt;&lt; fit &gt;&gt; and &lt;&lt; means &gt;&gt;.
knitr's options sit in the chunk header, so the script holds only R.

To run: Rscript growth.R, or make test. Rscript -e
'rmarkdown::render("growth.Rmd")' renders the paper.
</t>
<t tx="a.2" str_leo-rs-cell-ids="setup sta.20261009033628.8&#10;fit sta.20261009033628.10&#10;means sta.20261009033628.13&#10;"></t>
<t tx="sta.20261009033628.10">fit &lt;- aov(weight ~ group, data = PlantGrowth)
</t>
<t tx="sta.20261009033628.13">means &lt;- tapply(PlantGrowth$weight, PlantGrowth$group, mean)
round(means, 2)
</t>
<t tx="sta.20261009033628.14">@language r
&lt;&lt; setup &gt;&gt;
&lt;&lt; fit &gt;&gt;
print(summary(fit))
&lt;&lt; means &gt;&gt;
</t>
<t tx="sta.20261009033628.8">data(PlantGrowth)
</t>
</tnodes>
</leo_file>
