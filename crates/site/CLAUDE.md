# bimcraft-site

The site's geometry, `site`: polygons with their properties (`poly.rs`), the shapes a plot, building pads, roads and
car parks make (`shapes.rs`, `roads.rs`) and the car park's layout (`parking/`). Under `bimcraft-model`, over
`bimcraft-geom` only: it never names an element, so a change to the model does not rebuild it. The site's elements and
their checks (`masterplan`, `ground`, `carpark`) are the model's; they call into this crate for the shapes. The model
re-exports the module as `crate::site`, so code elsewhere names it as before.
