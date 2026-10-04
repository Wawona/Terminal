package com.aspauldingcode.wawona

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Compose text scroll for Wawona Terminal.
 * The VT screen stays in the Rust crate. This function only draws spans.
 */
@Composable
fun WawonaTerminalText(
    lines: List<AnnotatedString>,
    fontFamily: FontFamily,
    modifier: Modifier = Modifier,
) {
    val listState = rememberLazyListState()
    LaunchedEffect(lines.size) {
        if (lines.isNotEmpty()) {
            listState.scrollToItem(lines.lastIndex)
        }
    }
    LazyColumn(
        state = listState,
        modifier = modifier
            .fillMaxSize()
            .padding(horizontal = 12.dp)
            .graphicsLayer(),
    ) {
        items(lines) { line ->
            Text(
                text = line,
                fontFamily = fontFamily,
                fontSize = 13.sp,
                color = Color(0xFFE8E8E8),
            )
        }
    }
}

/** Unpack a little-endian `wwn_term_export` snapshot into styled lines. */
fun wawonaTerminalLines(packed: ByteArray?): List<AnnotatedString> {
    if (packed == null || packed.size < 4) {
        return listOf(AnnotatedString(""))
    }
    val buf = ByteBuffer.wrap(packed).order(ByteOrder.LITTLE_ENDIAN)
    val lineCount = buf.int
    val lines = ArrayList<AnnotatedString>(lineCount.coerceAtLeast(0))
    repeat(lineCount) {
        if (buf.remaining() < 4) {
            return@repeat
        }
        val cells = buf.int
        val pieces = ArrayList<IntArray>(cells)
        var written = 0
        while (written < cells && buf.remaining() >= 16) {
            val cp = buf.int
            val fg = buf.int
            val bg = buf.int
            val attrs = buf.int
            written++
            pieces.add(intArrayOf(cp, fg, bg, attrs))
        }
        var end = pieces.size
        while (end > 0) {
            val cell = pieces[end - 1]
            val cp = cell[0]
            val bg = cell[2]
            if ((cp == 0 || cp == ' '.code) && bg == 0) {
                end--
            } else {
                break
            }
        }
        val builder = AnnotatedString.Builder()
        for (index in 0 until end) {
            val cell = pieces[index]
            val cp = if (cell[0] == 0) ' '.code else cell[0]
            builder.pushStyle(wawonaTerminalSpan(cell[1], cell[2], cell[3]))
            builder.append(String(Character.toChars(cp)))
            builder.pop()
        }
        lines.add(builder.toAnnotatedString())
    }
    return if (lines.isEmpty()) listOf(AnnotatedString("")) else lines
}

private fun wawonaTerminalSpan(fg: Int, bg: Int, attrs: Int): SpanStyle {
    val color = Color(0xFF000000L or (fg.toLong() and 0xFFFFFF))
    val background = if (bg == 0) {
        Color.Transparent
    } else {
        Color(0xFF000000L or (bg.toLong() and 0xFFFFFF))
    }
    return SpanStyle(
        color = color,
        background = background,
        fontWeight = if (attrs and 1 != 0) FontWeight.Bold else FontWeight.Normal,
        fontStyle = if (attrs and 4 != 0) FontStyle.Italic else FontStyle.Normal,
        textDecoration = if (attrs and 8 != 0) TextDecoration.Underline else null,
    )
}
