# Attributes

In the [Iguazu data model](./data_model.md), attributes are found on entities and fields to add additional metadata influencing how the data is interpreted and displayed.

## `core:role`

Logical type that augments the entity type.

  * `"record"`: Group where children represent time-aligned columns; that is, the `n`th element of each child series is associated with the `n`th element of every other child series.
  * `"capture"`: Group where children represent independent series captured simultaneously, but not necessarily sampled at the same rate.
  * `"complex"`: Tuple with `re` and `im` fields representing a complex number.

## `core:text`

Text format template for records and structs.

This is a string containing `{name}` placeholders. For a `record` group or `bitstruct` field, the names refer to child entities / fields, which will be recursively formatted and substituted into the template. For leaf fields, the `{}` placeholder expands to the field-type-specific format that would otherwise have been used if this attribute weren't present.

## `time:rate`

Number of samples per second.

On a `timestamp`, this is the tick rate of the timestamp clock, used to map from timestamp values to real time.

On other types, this specifies that samples are evenly spaced in time at the specified sample rate. It is therefore mutually exclusive with `time:field`.

## `time:field`

On a `record` group, contains the name of the child field of type `timestamp` that holds the time of each sample.

## `time:epoch`

RFC 3339 timestamp representing the start time of data collection.

On a `timestamp`, this is the time represented by value `0`.

For other entities with a `time:rate` this is the time of the initial sample.

## `time:display`

* `iso` : ISO 8601 / RFC 3339 absolute timestamp
* `relative` : Relative time in HH:MM:SS.sss format
* `raw`: Integer sample number

Defaults to `iso` if `time:rate` and `time:epoch` are specified, `relative` if `time:rate` is specified, otherwise `raw`.

## `number:scale`

Scale factor multiplied with the stored value. This is used to scale data stored in fixed-point format.

Default: 1.0

## `number:offset`

Offset applied to the number after scaling. This is used for fixed-point representations with a bias or offset.

Default: 0.0

## `number:min`

Logical minimum value. Used for axis bounds.

## `number:max`

Logical maximum value. Used for axis bounds.

## `display:layout`

Default view to display this data.

  * `{"view": "timeline"}`
  * `{"view": "table"}`

## `display:color`

Accent color to distinguish this entity from others. Used as the color of the line or other timeline mark.

  * `neutral` (White / Black depending on theme)
  * `brown`
  * `red`
  * `orange`
  * `yellow`
  * `green`
  * `blue`
  * `purple`

## `display:timeline:row`

How this entity should be displayed on the timeline.

  * `hidden`
  * `stack` : Children are displayed as separate timeline rows.
  * `yaxis` : Analog Y axis. If applied to an entity with children, the children are plotted on a shared Y axis.
  * `trace` : A row showing contiguous runs of the same value with the value displayed as text.
  * `logic` : Single-bit value displayed as a logic trace.
  * `events` : Each value is displayed as a discrete event.
