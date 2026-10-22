const { invoke } = window.__TAURI__.core;

const utf8d = new TextDecoder();
const load_all = async function () {
  const mzml_l = d3.tsvParseRows(utf8d.decode(await invoke("mzml_tsv")));
  const tar_list = await invoke("tar_list", { nfiles: mzml_l.length });
  const mzml_sel = document.getElementById("mzml_sel");


  const tar_sel = document.getElementById("tar");

  const mzmlFrag = document.createDocumentFragment();
  for (let i = 0; i < mzml_l.length; i++) {
    mzmlFrag.appendChild(new Option(mzml_l[i][0], i));
  }
  mzml_sel.appendChild(mzmlFrag);

  const tarFrag = document.createDocumentFragment();
  for (let i = 0; i < tar_list.length; i++) {
    tarFrag.appendChild(new Option(`${i + 1}. ${tar_list[i].name}`, i));
  }
  tar_sel.appendChild(tarFrag);

  mzml_sel.value = mzml_sel.options[0].value;
  tar_sel.value = tar_sel.options[0].value;
  gen(mzml_l, tar_list, mzml_sel.value, tar_sel.value);
  mzml_sel.onchange = function () {
    animateValueChange(document.getElementById("print_mzml"));
    gen(mzml_l, tar_list, mzml_sel.value, tar_sel.value);
  };
  tar_sel.onchange = function () {
    animateValueChange(document.getElementById("print_analyte"));
    gen(mzml_l, tar_list, mzml_sel.value, tar_sel.value);
  };
};

window.onload = async function () {
  load_all();
};

const width = 854;
const height1 = 180;
const marginTop1 = 20;
const marginRight1 = 30;
const marginBottom1 = 20;
const marginLeft1 = 40;
async function gen(mzml_l, tar_list, mzml_sel, tar_sel) {
  const container = document.getElementById("container");
  container.innerHTML = "";
  const mzml_f = mzml_l[mzml_sel][0];
  document.getElementById("print_mzml").innerHTML = mzml_f;
  const tar = tar_list[tar_sel];
  document.getElementById("print_analyte").innerHTML =
    `${parseInt(tar_sel) + 1}. ${tar.name}`;
  const mz_rt_sc = tar.mz_rt_sc[mzml_sel];
  const svg_spec = new Array(tar.f_mz_l.length + 1)
    .fill(null)
    .map(() => d3.create("svg"));
  if (mz_rt_sc.pres) {
    print_mirror(mz_rt_sc, tar.f_mz_l);
  }
  const [chrom, chrom2] = await invoke("get_ms1", {
    bn: mzml_f,
    mzRtSc: mz_rt_sc,
    rtwid: 0.5,
    fMzL: tar.f_mz_l.map((d) => d[0]),
  });
  const shape_s = mz_rt_sc.pres
    ? `, SHAPE: ${d3.format(".2f")(mz_rt_sc.shape)}, SN: ${d3.format(".2f")(mz_rt_sc.smooth)}`
    : "";
  print_xic(
    svg_spec[0],
    chrom,
    mz_rt_sc,
    mz_rt_sc.auc,
    `PRECURSOR: ${d3.format(".3f")(mz_rt_sc.mz)}m/z${shape_s}`,
  );
  tar.f_mz_l.forEach(([f_mz], i) => {
    const cor_s = mz_rt_sc.pres
      ? ", CORRELATION: " + d3.format(".2f")(mz_rt_sc.cor_l[i][0])
      : "";
    print_xic(
      svg_spec[i + 1],
      chrom2[i],
      mz_rt_sc,
      mz_rt_sc.pres ? mz_rt_sc.cor_l[i][1] : 0,
      `FRAGMENT: ${d3.format(".3f")(f_mz)}m/z${cor_s}`,
    );
  });
  for (const spec of svg_spec) {
    container.append(spec.node());
  }
}
function animateValueChange(element) {
  element.animate(
    {
      backgroundColor: ["#ff0000BF", "#ff000000"],
    },
    {
      duration: 2000,
    },
  );
}
function print_xic(svg, chrom, mz_rt_sc, auc, title) {
  const feat_rt = mz_rt_sc.rt;
  const int_wid = mz_rt_sc.sc;

  if (!mz_rt_sc.pres) {
    const chrom_ = chrom.filter(
      (d) => feat_rt - int_wid < d[0] && d[0] < feat_rt + int_wid,
    );
    auc =
      d3.sum(
        chrom_.slice(1).map(([rt1, i1], i) => {
          const [rt0, i0] = chrom_[i];
          return (i0 + i1) * (rt1 - rt0);
        }),
      ) * 30.0;
  }
  svg
    .append("text")
    .attr("x", "99%")
    .attr("text-anchor", "end")
    .attr("dominant-baseline", "text-before-edge")
    .text(
      `AREA: ${d3.format(".1f")(auc)}, RT: ${d3.format(".2f")(mz_rt_sc.rt)}`,
    );

  svg
    .attr("width", width)
    .attr("height", height1)
    .style("background", "white")
    .style("border", "solid")
    .style("border-radius", "1em");
  svg
    .append("text")
    .attr("x", "50%")
    .attr("text-anchor", "middle")
    .attr("dominant-baseline", "text-before-edge")
    .text(title);
  const x = d3
    .scaleLinear()
    .domain([chrom[0][0], chrom[chrom.length - 1][0]])
    .range([marginLeft1, width - marginRight1]);
  const y = d3
    .scaleLinear()
    .domain([0, 1.05 * d3.max(chrom, (d) => d[1])])
    .range([height1 - marginBottom1, marginTop1]);
  svg
    .append("g")
    .attr("transform", `translate(0,${height1 - marginBottom1})`)
    .call(d3.axisBottom(x))
    .call((g) => g.select(".domain").attr("opacity", 0.5))
    .call((g) => g.attr("font-size", 12));
  svg
    .append("g")
    .attr("transform", `translate(${marginLeft1})`)
    .call(d3.axisLeft(y).ticks(2, "s"))
    .call((g) => g.select(".domain").remove())
    .call((g) => g.attr("font-size", 12));
  svg
    .append("g")
    .selectAll("circle")
    .data(chrom)
    .join("circle")
    .attr("cx", (d) => x(d[0]))
    .attr("cy", (d) => y(d[1]))
    .attr("r", 2);
  const int_begin = x(feat_rt - int_wid);
  svg
    .append("rect")
    .attr("width", x(feat_rt + int_wid) - int_begin)
    .attr("height", y.range()[0] - y.range()[1])
    .attr("x", int_begin)
    .attr("y", y.range()[1])
    .attr("opacity", 0.1);
  const line = d3
    .line()
    .x((d) => x(d[0]))
    .y((d) => y(d[1]));
  svg
    .append("path")
    .attr("d", line(chrom))
    .attr("fill", "none")
    .attr("stroke", "black");
}

function print_mirror(mz_rt_sc, ent_mz_i) {
  const svg = d3
    .create("svg")
    .attr("width", width)
    .attr("height", height1)
    .style("background", "white")
    .style("border", "solid")
    .style("border-radius", "1em");
  svg
    .append("text")
    .attr("x", "50%")
    .attr("text-anchor", "middle")
    .attr("dominant-baseline", "text-before-edge")
    .attr("font-weight", "bold")
    .attr("fill", "red")
    .text("area ratio");
  svg
    .append("text")
    .attr("x", "50%")
    .attr("y", "100%")
    .attr("text-anchor", "middle")
    .attr("font-weight", "bold")
    .attr("dominant-baseline", "ideographic")
    .attr("fill", "blue")
    .text("library spectrum");
  const x = d3
    .scaleLinear()
    .domain([0, mz_rt_sc.mz + 0.3])
    .range([marginLeft1, width - marginRight1]);
  const ymax1 = d3.max(mz_rt_sc.cor_l, (d) => d[1]);
  const ymax0 = d3.max(ent_mz_i, (d) => d[1]);
  const y = d3
    .scaleLinear()
    .domain([-ymax0, 0, ymax1])
    .range([height1 - marginBottom1 - 5, height1 / 2, marginTop1 + 5]);
  const exp_spec = mz_rt_sc.cor_l.map((d, i) => [ent_mz_i[i][0], d[1]]);
  const lib_spec = ent_mz_i.map((d) => [d[0], -d[1]]);
  const all_p = exp_spec.concat(lib_spec);
  svg
    .append("g")
    .attr("stroke-width", 2)
    .selectAll("line")
    .data(all_p)
    .join("line")
    .attr("x1", (d) => x(d[0]))
    .attr("y1", y(0))
    .attr("x2", (d) => x(d[0]))
    .attr("y2", (d) => y(d[1]))
    .attr("stroke", (d) => (d[1] > 0 ? "red" : "blue"));
  svg
    .append("g")
    .selectAll("circle")
    .data(all_p)
    .join("circle")
    .attr("cx", (d) => x(d[0]))
    .attr("cy", (d) => y(d[1]))
    .attr("fill", (d) => (d[1] > 0 ? "red" : "blue"))
    .attr("r", 3);
  const bisect = d3.bisector((d) => d[0]).center;
  const tt = svg.append("g").style("display", "none");
  const tt1 = tt.append("g");
  tt1
    .append("rect")
    .attr("width", 70)
    .attr("height", 20)
    .attr("fill", "red")
    .attr("x", -tt1.select("rect").attr("width") / 2)
    .attr("y", -24)
    .attr("rx", 4)
    .attr("ry", 4);
  tt1
    .append("text")
    .attr("y", -14)
    .attr("fill", "white")
    .attr("text-anchor", "middle")
    .attr("dominant-baseline", "central");
  const tt2 = tt.append("g");
  tt2
    .append("rect")
    .attr("width", 70)
    .attr("height", 20)
    .attr("fill", "blue")
    .attr("x", -tt2.select("rect").attr("width") / 2)
    .attr("y", 4)
    .attr("rx", 4)
    .attr("ry", 4);
  tt2
    .append("text")
    .attr("y", 14)
    .attr("fill", "white")
    .attr("text-anchor", "middle")
    .attr("dominant-baseline", "central");
  svg
    .on("pointerenter pointermove", (event) => {
      const i = bisect(ent_mz_i, x.invert(d3.pointer(event)[0]));
      tt.style("display", null)
        .selectAll("text")
        .text(d3.format(".3f")(ent_mz_i[i][0]));
      tt1.attr(
        "transform",
        `translate(${x(ent_mz_i[i][0])}, ${y(exp_spec[i][1])})`,
      );
      tt2.attr(
        "transform",
        `translate(${x(ent_mz_i[i][0])}, ${y(lib_spec[i][1])})`,
      );
    })
    .on("pointerleave", () => tt.style("display", "none"));
  svg
    .append("line")
    .attr("y1", y(0))
    .attr("x2", "100%")
    .attr("y2", y(0))
    .attr("opacity", 0.5)
    .attr("stroke", "black");
  const container = document.getElementById("container");
  container.append(svg.node());
}
