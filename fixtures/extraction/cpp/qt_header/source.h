// SPDX-FileCopyrightText: 2026 The Kirigami Authors
// SPDX-License-Identifier: LGPL-2.0-or-later

#pragma once

#include <QObject>
#include <QPointF>

class ColumnView;

class KIRIGAMI2_EXPORT ColumnViewAttached : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_ATTACHED(ColumnViewAttached)

    Q_PROPERTY(int index READ index WRITE setIndex NOTIFY indexChanged FINAL)
    Q_PROPERTY(ColumnView *view READ view NOTIFY viewChanged FINAL)
    Q_PROPERTY(QPointF origin
               MEMBER origin
               CONSTANT FINAL)

public:
    enum class ColumnResizeMode {
        FixedColumns,
        DynamicColumns,
    };
    Q_ENUM(ColumnResizeMode)

    explicit ColumnViewAttached(QObject *parent = nullptr);
    ~ColumnViewAttached() override;

    int index() const;
    void setIndex(int index);

    QQuickItem *contentItem() const;
    const QString &name() const;
    QList<int> *items();
    static ColumnViewAttached *instance();

    Q_INVOKABLE void reset();

public Q_SLOTS:
    void refresh();

Q_SIGNALS:
    void indexChanged();
    void viewChanged(ColumnView *view);

private:
    int m_index = 0;
};

class KIRIGAMI2_EXPORT ScrollIntentionEvent : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QPointF delta MEMBER delta CONSTANT FINAL)

public:
    QPointF delta;
};

struct Geometry {
    int width;
};
