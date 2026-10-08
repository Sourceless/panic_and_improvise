Feature: The magazine, the bolt, and reloading

  Background:
    Given I am in test room 1
    And I have the smg

  Scenario: A full magazine holds thirty rounds
    Then the magazine has 30 rounds

  Scenario: Each shot uses a round
    When I fire 3 single shots
    Then the magazine has 27 rounds

  Scenario: The bolt rests to the rear on a ready open-bolt gun
    Then the bolt is back

  Scenario: R reloads a part-empty magazine, with the gun dipping off the screen
    Given I fire 3 single shots
    When I press R
    Then the gun is reloading
    And the gun has played 1 reload sound, 0 of them with the bolt charged
    And the reload takes 2 seconds
    And the gun has dipped off the screen
    And the gun is back up
    And the magazine has 30 rounds

  Scenario: R does nothing with a full magazine
    When I press R
    Then the gun is not reloading

  Scenario: You cannot fire during a reload
    Given I fire once
    And I press R
    And the gun is reloading
    When I hold fire for 0.5 seconds
    Then the gun fired between 1 and 1 shots

  Scenario: Reloading takes the gun off its sights
    Given I right click
    And the gun is on its sights
    And I fire once
    When I press R
    Then the gun is back at the hip

  Scenario: Emptying the magazine leaves the bolt forward, and nothing reloads by itself
    When I hold fire for 4 seconds
    Then the gun fired between 30 and 30 shots
    And the magazine has 0 rounds
    And the bolt is forward
    And the gun is not reloading

  Scenario: A dry gun clicks, and does nothing else, when the trigger is pulled
    Given I hold fire for 4 seconds
    When I fire once
    Then the gun fired between 30 and 30 shots
    And the gun has clicked 1 time
    And the gun is not reloading

  Scenario: Reloading a dry gun takes longer, because the bolt has to be charged
    Given I hold fire for 4 seconds
    And the bolt is forward
    When I press R
    Then the gun is reloading
    And the gun has played 1 reload sound, 1 of them with the bolt charged
    And the reload takes 2.7 seconds
    And the gun is back up
    And the magazine has 30 rounds
    And the bolt is back

  Scenario: Once reloaded the gun fires again
    Given I hold fire for 4 seconds
    And I press R
    And the gun is back up
    When I fire once
    Then the gun fired between 31 and 31 shots

  Scenario: Mashing the trigger on a dry gun doesn't rattle
    Given I hold fire for 4 seconds
    When I pull the trigger 4 times quickly
    Then the gun has clicked 1 time

  Scenario: Sprinting abandons a reload, and the magazine stays as it was
    Given I fire 3 single shots
    And I press R
    And the gun is reloading
    When I hold W
    And I hold Shift
    And I wait 1 second
    Then the gun is not reloading
    And the magazine has 27 rounds

  Scenario: Reloading while sprinting drops the sprint and reloads
    Given I fire 3 single shots
    And I hold W
    And I hold Shift
    And I am moving faster than 8 metres per second
    When I press R
    Then the gun is reloading
    When I wait 0.5 seconds
    Then the gun is reloading
    And I am moving slower than 8 metres per second

  Scenario: Jumping abandons a reload
    Given I fire 3 single shots
    And I press R
    And the gun is reloading
    When I press Space
    Then the gun is not reloading
    And the magazine has 27 rounds
