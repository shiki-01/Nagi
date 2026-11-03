package com.shiki01.nagi

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import androidx.core.graphics.PathParser
import java.io.File
import java.io.IOException

/**
 * キーに描く Iconify アイコン。[IconShape] の path data を [Path] にしたもの。
 * 単色で描く（色は呼び出し側が渡す文字色）。viewBox に合わせて拡縮し、指定の正方形の中央に置く。
 */
internal class IconGlyph private constructor(
    private val shape: IconShape,
    private val items: List<Item>,
) {
    private class Item(val path: Path, val src: IconPath)

    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)

    /** [cx], [cy] を中心に、長辺が [side] px になる大きさで描く */
    fun draw(canvas: Canvas, cx: Float, cy: Float, side: Float, color: Int) {
        val w = shape.width.toFloat()
        val h = shape.height.toFloat()
        val scale = side / maxOf(w, h)
        if (!(scale > 0f) || scale.isInfinite()) return
        val save = canvas.save()
        canvas.translate(cx - w * scale / 2f, cy - h * scale / 2f)
        canvas.scale(scale, scale)
        canvas.translate(-shape.left.toFloat(), -shape.top.toFloat())
        paint.color = color
        for (item in items) {
            val p = item.src
            if (p.fill) {
                paint.style = Paint.Style.FILL
                paint.strokeWidth = 0f
                canvas.drawPath(item.path, paint)
            }
            if (p.stroke) {
                paint.style = Paint.Style.STROKE
                paint.strokeWidth = p.strokeWidth.toFloat()
                paint.strokeCap = when (p.cap) {
                    "round" -> Paint.Cap.ROUND
                    "square" -> Paint.Cap.SQUARE
                    else -> Paint.Cap.BUTT
                }
                paint.strokeJoin = when (p.join) {
                    "round" -> Paint.Join.ROUND
                    "bevel" -> Paint.Join.BEVEL
                    else -> Paint.Join.MITER
                }
                canvas.drawPath(item.path, paint)
            }
        }
        canvas.restoreToCount(save)
    }

    companion object {
        /** [shape] から作る。path data を 1 本も読めなければ null */
        fun from(shape: IconShape): IconGlyph? {
            val items = ArrayList<Item>()
            for (p in shape.paths) {
                val parsed: Path? = try {
                    PathParser.createPathFromPathData(p.d)
                } catch (e: RuntimeException) {
                    null
                }
                val path = parsed ?: continue
                path.fillType = if (p.evenOdd) Path.FillType.EVEN_ODD else Path.FillType.WINDING
                items.add(Item(path, p))
            }
            return if (items.isEmpty()) null else IconGlyph(shape, items)
        }
    }
}

/**
 * 取り込んだ Iconify アイコンの置き場（`filesDir/layouts/icons/<prefix>__<name>.json`。保存形式は [IconifyLogic.encode]）。
 * 読み込み結果はメモリにキャッシュする（キーボードの組み立てでファイルを読む量を最小にする）。
 * 保存のたびにキャッシュを捨てる。保存はワーカースレッドで行うこと
 */
internal object IconStore {
    private const val DIR = "layouts/icons"

    private val cache = HashMap<String, IconGlyph?>()

    private fun dir(ctx: Context): File = File(ctx.filesDir, DIR)

    /** 保存済みか（ファイルがあるか。中身は確かめない） */
    fun exists(ctx: Context, ref: IconRef): Boolean = File(dir(ctx), ref.fileName).isFile

    /** [ref] のアイコン。無い・壊れている・描けないときは null（結果、null もキャッシュする） */
    fun load(ctx: Context, ref: IconRef): IconGlyph? {
        synchronized(cache) {
            if (cache.containsKey(ref.id)) return cache[ref.id]
        }
        val glyph = try {
            val f = File(dir(ctx), ref.fileName)
            if (f.isFile) IconifyLogic.decode(f.readText(Charsets.UTF_8))?.let { IconGlyph.from(it) } else null
        } catch (e: IOException) {
            null
        }
        synchronized(cache) { cache[ref.id] = glyph }
        return glyph
    }

    /** [shape] を原子的に保存する。成功なら true */
    fun save(ctx: Context, ref: IconRef, shape: IconShape): Boolean {
        val d = dir(ctx)
        if (!d.isDirectory && !d.mkdirs()) return false
        val target = File(d, ref.fileName)
        val tmp = File(d, ref.fileName + ".tmp")
        try {
            tmp.writeText(IconifyLogic.encode(shape), Charsets.UTF_8)
            if (!tmp.renameTo(target)) {
                target.delete()
                if (!tmp.renameTo(target)) {
                    tmp.delete()
                    return false
                }
            }
        } catch (e: IOException) {
            tmp.delete()
            return false
        }
        synchronized(cache) { cache.clear() }
        return true
    }
}
