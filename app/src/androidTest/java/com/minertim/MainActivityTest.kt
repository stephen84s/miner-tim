package com.minertim

import androidx.test.ext.junit.rules.ActivityScenarioRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.espresso.Espresso.onView
import androidx.test.espresso.assertion.ViewAssertions.matches
import androidx.test.espresso.matcher.ViewMatchers.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MainActivityTest {

    @get:Rule
    val activityRule = ActivityScenarioRule(MainActivity::class.java)

    @Test
    fun activityLaunches() {
        onView(withId(R.id.btnStartStop))
            .check(matches(isDisplayed()))
    }

    @Test
    fun walletAddressFieldIsDisplayed() {
        onView(withId(R.id.etWalletAddress))
            .check(matches(isDisplayed()))
    }

    @Test
    fun poolAddressFieldIsDisplayed() {
        onView(withId(R.id.etPoolAddress))
            .check(matches(isDisplayed()))
    }
}
