package com.aspauldingcode.wawona

import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Text
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray

/** One screen row. `id` stays with the text so LazyColumn does not reuse the wrong row. */
data class WawonaTerminalRow(
    val id: Int,
    val line: AnnotatedString,
)

/**
 * Compose text scroll for Wawona Terminal.
 * The VT screen stays in the Rust crate. This function only draws spans.
 * Host monospace is the face. Nerd Font is the private-use fallback inside spans.
 */
@Composable
fun WawonaTerminalText(
    rows: List<WawonaTerminalRow>,
    minRowHeight: Dp,
    modifier: Modifier = Modifier,
) {
    val listState = rememberLazyListState()
    val blink = rememberInfiniteTransition(label = "wwn-term-blink")
    val blinkAlpha by blink.animateFloat(
        initialValue = 1f,
        targetValue = 0.15f,
        animationSpec = infiniteRepeatable(tween(durationMillis = 500), RepeatMode.Reverse),
        label = "wwn-term-blink-alpha",
    )
    LaunchedEffect(rows.size) {
        if (rows.isNotEmpty()) {
            listState.scrollToItem(rows.lastIndex)
        }
    }
    LazyColumn(
        state = listState,
        modifier = modifier
            .fillMaxSize()
            .padding(horizontal = 12.dp)
            .graphicsLayer(),
    ) {
        items(rows, key = { it.id }) { row ->
            Text(
                text = if (row.line.text.isEmpty()) {
                    AnnotatedString(" ")
                } else {
                    fadeBlink(row.line, blinkAlpha)
                },
                fontFamily = FontFamily.Monospace,
                fontSize = 13.sp,
                color = Color(0xFFE8E8E8),
                softWrap = false,
                maxLines = 1,
                overflow = TextOverflow.Clip,
                modifier = Modifier.defaultMinSize(minHeight = minRowHeight),
            )
        }
    }
}

object WawonaTerminalRowIds {
    private var next = 1

    fun align(old: List<WawonaTerminalRow>, lines: List<AnnotatedString>): List<WawonaTerminalRow> {
        val oldKeys = old.map { it.line.text }
        val newKeys = lines.map { it.text }
        val ids = idsFor(old.map { it.id }, oldKeys, newKeys)
        return ids.zip(lines) { id, line -> WawonaTerminalRow(id, line) }
    }

    private fun mint(): Int {
        val id = next
        next += 1
        return id
    }

    private fun idsFor(oldIDs: List<Int>, oldKeys: List<String>, newKeys: List<String>): List<Int> {
        if (newKeys == oldKeys && newKeys.size == oldIDs.size) {
            return oldIDs
        }
        if (newKeys.size >= oldKeys.size && newKeys.subList(0, oldKeys.size) == oldKeys) {
            val ids = oldIDs.toMutableList()
            while (ids.size < newKeys.size) {
                ids.add(mint())
            }
            return ids
        }
        if (oldKeys.isNotEmpty() && newKeys.size == oldKeys.size &&
            newKeys.dropLast(1) == oldKeys.drop(1)
        ) {
            val ids = oldIDs.drop(1).toMutableList()
            ids.add(mint())
            return ids
        }
        val overlap = minOf(oldIDs.size, newKeys.size)
        val ids = ArrayList<Int>(newKeys.size)
        repeat(newKeys.size - overlap) { ids.add(mint()) }
        ids.addAll(oldIDs.takeLast(overlap))
        return ids
    }
}

/** Draw UniFFI `TerminalSession.lines()` JSON. Compose does not group cells. */
fun wawonaTerminalLines(document: String?, nerd: FontFamily): List<AnnotatedString> {
    if (document.isNullOrEmpty()) {
        return listOf(AnnotatedString(""))
    }
    val array = try {
        JSONArray(document)
    } catch (_: org.json.JSONException) {
        return listOf(AnnotatedString(document))
    }
    val lines = ArrayList<AnnotatedString>(array.length())
    for (index in 0 until array.length()) {
        val spans = array.optJSONObject(index)?.optJSONArray("spans") ?: continue
        val builder = AnnotatedString.Builder()
        for (spanIndex in 0 until spans.length()) {
            val span = spans.optJSONObject(spanIndex) ?: continue
            val text = span.optString("text")
            if (text.isEmpty()) {
                continue
            }
            appendSplit(
                builder,
                text,
                wawonaTerminalSpan(
                    span.optInt("fg"),
                    span.optInt("bg"),
                    span.optBoolean("bold"),
                    span.optBoolean("italic"),
                    span.optBoolean("underline"),
                    span.optBoolean("strike"),
                ),
                nerd,
                span.optBoolean("blink"),
            )
        }
        lines.add(builder.toAnnotatedString())
    }
    return if (lines.isEmpty()) listOf(AnnotatedString("")) else lines
}

private fun appendSplit(
    builder: AnnotatedString.Builder,
    text: String,
    style: SpanStyle,
    nerd: FontFamily,
    blink: Boolean,
) {
    var start = 0
    var index = 0
    var nerdRun: Boolean? = null
    while (index < text.length) {
        val point = text.codePointAt(index)
        val flag = isNerd(point)
        if (nerdRun != null && nerdRun != flag) {
            pushRun(builder, text.substring(start, index), style, nerdRun, nerd, blink)
            start = index
        }
        nerdRun = flag
        index += Character.charCount(point)
    }
    if (start < text.length) {
        pushRun(builder, text.substring(start), style, nerdRun == true, nerd, blink)
    }
}

private fun pushRun(
    builder: AnnotatedString.Builder,
    text: String,
    style: SpanStyle,
    nerdRun: Boolean,
    nerd: FontFamily,
    blink: Boolean,
) {
    val start = builder.length
    builder.pushStyle(
        style.copy(fontFamily = if (nerdRun) nerd else FontFamily.Monospace),
    )
    builder.append(text)
    builder.pop()
    if (blink) {
        builder.addStringAnnotation("wwn-blink", "1", start, builder.length)
    }
}

private fun fadeBlink(line: AnnotatedString, alpha: Float): AnnotatedString {
    val marks = line.getStringAnnotations("wwn-blink", 0, line.length)
    if (marks.isEmpty() || alpha >= 0.99f) {
        return line
    }
    val builder = AnnotatedString.Builder(line)
    for (mark in marks) {
        val styles = line.spanStyles.filter { it.start < mark.end && it.end > mark.start }
        for (style in styles) {
            val from = maxOf(style.start, mark.start)
            val to = minOf(style.end, mark.end)
            builder.addStyle(
                style.item.copy(color = style.item.color.copy(alpha = alpha)),
                from,
                to,
            )
        }
    }
    return builder.toAnnotatedString()
}

private fun isNerd(point: Int): Boolean {
    return point in 0xE000..0xF8FF ||
        point in 0xF0000..0xFFFFD ||
        point in 0x100000..0x10FFFD
}

private fun wawonaTerminalSpan(
    fg: Int,
    bg: Int,
    bold: Boolean,
    italic: Boolean,
    underline: Boolean,
    strike: Boolean,
): SpanStyle {
    val color = Color(0xFF000000L or (fg.toLong() and 0xFFFFFF))
    val background = if (bg == 0) {
        Color.Transparent
    } else {
        Color(0xFF000000L or (bg.toLong() and 0xFFFFFF))
    }
    return SpanStyle(
        color = color,
        background = background,
        fontWeight = if (bold) FontWeight.Bold else FontWeight.Normal,
        fontStyle = if (italic) FontStyle.Italic else FontStyle.Normal,
        textDecoration = when {
            underline && strike -> TextDecoration.combine(
                listOf(TextDecoration.Underline, TextDecoration.LineThrough),
            )
            underline -> TextDecoration.Underline
            strike -> TextDecoration.LineThrough
            else -> null
        },
    )
}
